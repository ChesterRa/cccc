use super::{
    hub::Hub,
    protocol::{self, Request},
};
use anyhow::{Context, Result};
use cccc_contracts::{DaemonRequest, session_node::*};
use cccc_core::session_nodes;
use iroh::endpoint::Connection;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, task::JoinHandle};

pub(super) async fn daemon(hub: &Hub, op: &str, args: Value) -> Result<Value> {
    let response = hub
        .daemon
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args
                .as_object()
                .cloned()
                .context("Expected an argument object")?,
        })
        .await?;
    anyhow::ensure!(response.ok, "Native receive channel is unavailable");
    Ok(Value::Object(response.result))
}

pub(super) async fn sync_receivers(
    hub: Arc<Hub>,
    node_id: &str,
    epoch: &str,
    connection: &Connection,
    receivers: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    sources: Vec<RemoteSessionSource>,
) {
    let mut receivers = receivers.lock().await;
    receivers.retain(|_, task| !task.is_finished());
    for source in sources
        .into_iter()
        .filter(|source| source.alive && source.source.connected)
    {
        if receivers.contains_key(&source.source.source_id) {
            continue;
        }
        let key = source.source.source_id.clone();
        let connection = connection.clone();
        let hub = Arc::clone(&hub);
        let node_id = node_id.to_owned();
        let epoch = epoch.to_owned();
        receivers.insert(
            key,
            tokio::spawn(async move {
                receiver(hub, source, node_id, epoch, connection).await;
            }),
        );
    }
}

async fn receiver(
    hub: Arc<Hub>,
    source: RemoteSessionSource,
    node_id: String,
    epoch: String,
    connection: Connection,
) {
    let id = format!("{}:{}", epoch, uuid::Uuid::new_v4());
    let args = json!({"source_id":source.source.source_id,"connection_id":id});
    let mut attached = false;
    let mut heartbeat = tokio::time::Instant::now() - Duration::from_secs(10);
    loop {
        tokio::select! { _ = connection.closed() => break, _ = tokio::time::sleep(Duration::from_millis(400)) => {} }
        if !hub.current(&node_id, &epoch).await
            || session_nodes::source(&hub.home, &source.source.source_id).is_err()
        {
            break;
        }
        if !attached || heartbeat.elapsed() >= Duration::from_secs(5) {
            let op = if attached {
                "local_session_receiver_heartbeat"
            } else {
                "local_session_receiver_attach"
            };
            if daemon(&hub, op, args.clone()).await.is_err() {
                attached = false;
                continue;
            }
            attached = true;
            heartbeat = tokio::time::Instant::now();
        }
        let next = match daemon(&hub, "local_session_receiver_next", args.clone()).await {
            Ok(next) => next,
            Err(_) => {
                attached = false;
                continue;
            }
        };
        if next["handoff"].is_null() {
            continue;
        }
        let handoff = &next["handoff"];
        let outcome = transfer(&hub, &node_id, &connection, &source, handoff).await.unwrap_or(SessionNodeDeliveryOutcome::Ambiguous { reason: "Remote native handoff was offered but its result is unknown; automatic replay is blocked".into() });
        let (state, reason) = match outcome {
            SessionNodeDeliveryOutcome::Accepted => ("accepted", String::new()),
            SessionNodeDeliveryOutcome::Rejected { reason } => ("failed", reason),
            SessionNodeDeliveryOutcome::Ambiguous { reason } => ("ambiguous", reason),
            // A definite, pre-submission negative result can use the normal
            // local transport unavailable state; never infer it from a timeout.
            SessionNodeDeliveryOutcome::Unavailable => ("unavailable", String::new()),
        };
        let mut ack = args.clone();
        ack["handoff_id"] = handoff["handoff_id"].clone();
        ack["state"] = json!(state);
        ack["reason"] = json!(reason);
        let _ = daemon(&hub, "local_session_receiver_ack", ack).await;
    }
    let _ = daemon(&hub, "local_session_receiver_detach", args).await;
}

async fn transfer(
    hub: &Hub,
    node_id: &str,
    connection: &Connection,
    source: &RemoteSessionSource,
    handoff: &Value,
) -> Result<SessionNodeDeliveryOutcome> {
    let group_id = handoff["group_id"]
        .as_str()
        .context("Handoff Group missing")?;
    let membership = session_nodes::memberships(&hub.home, node_id)?
        .into_iter()
        .find(|membership| {
            membership.group_id == group_id
                && membership.source_id == source.native_source_id
                && membership.actor_id == handoff["actor_id"].as_str().unwrap_or_default()
                && membership.actor_generation
                    == handoff["actor_generation"].as_str().unwrap_or_default()
                && membership.enabled
        })
        .context("Membership changed before native handoff")?;
    let hub_id = membership.hub_id.clone();
    let text = format!(
        "[CCCC remote session · hub_id={hub_id}]\nUse this hub_id and the explicit group_id for CCCC tools. Work in your original native session on this machine; files, commands, credentials and approvals remain here.\n\n{}",
        handoff["text"].as_str().context("Handoff text missing")?
    );
    let delivery = RemoteSessionDelivery {
        handoff_id: handoff["handoff_id"]
            .as_str()
            .context("Handoff ID missing")?
            .into(),
        source_id: source.native_source_id.clone(),
        source_revision: membership.source_revision.clone(),
        membership,
        event_ids: serde_json::from_value(handoff["event_ids"].clone())?,
        text,
    };
    let response = protocol::call(
        connection,
        &Request::Deliver {
            delivery: Box::new(delivery),
        },
    )
    .await?;
    Ok(serde_json::from_value(response["outcome"].clone())?)
}

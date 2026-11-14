use super::protocol::{self, Request};
use anyhow::{Context, Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, local_session::LocalSessionSource, session_node::*};
use cccc_core::{HomeLayout, local_sessions, session_nodes};
use iroh::endpoint::Connection;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Semaphore, task::JoinSet};

pub(super) async fn run_session(
    home: &HomeLayout,
    daemon: &DaemonClient,
    secret: &str,
    connection: &Connection,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let permits = Arc::new(Semaphore::new(8));
    let mut calls = JoinSet::new();
    loop {
        tokio::select! {
            _ = connection.closed() => return Ok(()),
            Some(_) = calls.join_next(), if !calls.is_empty() => {},
            _ = interval.tick() => {
                let sources = sources(home, daemon).await?;
                let response = protocol::call(connection, &Request::Sync { sources }).await?;
                session_nodes::update_memberships(home, secret, true, serde_json::from_value(response["memberships"].clone())?)?;
            }
            incoming = connection.accept_bi() => {
                let (mut send, mut recv) = incoming?;
                let permit = Arc::clone(&permits).try_acquire_owned()?;
                let home = home.clone(); let daemon = daemon.clone(); let secret = secret.to_owned();
                calls.spawn(async move {
                    let _permit = permit;
                    let response = incoming_delivery(&home, &daemon, &secret, protocol::read(&mut recv).await).await.unwrap_or_else(|_| json!({"error":"Native handoff authority is unavailable"}));
                    let _ = protocol::write(&mut send, &response).await;
                });
            }
        }
    }
}

async fn incoming_delivery(
    home: &HomeLayout,
    daemon: &DaemonClient,
    secret: &str,
    request: Result<Request>,
) -> Result<Value> {
    let Request::Deliver { delivery } = request? else {
        anyhow::bail!("Unexpected hub request");
    };
    let client = session_nodes::client(home)?
        .filter(|client| client.secret == secret)
        .context("Connector unavailable")?;
    ensure!(
        delivery.membership.hub_id == client.endpoint.endpoint_id && delivery.membership.enabled,
        "Unexpected hub membership"
    );
    local_sessions::validate_source(home, &delivery.source_id, &delivery.source_revision)?;
    let mut memberships = client.memberships;
    memberships.retain(|member| {
        member.source_id != delivery.source_id || member.group_id != delivery.membership.group_id
    });
    memberships.push(delivery.membership.clone());
    session_nodes::update_memberships(home, secret, true, memberships)?;
    let response = daemon
        .call(&DaemonRequest {
            v: 1,
            op: "session_node_incoming".into(),
            args: json!({"secret":secret,"delivery":delivery})
                .as_object()
                .expect("incoming arguments")
                .clone(),
        })
        .await?;
    ensure!(response.ok, "Native session rejected the handoff");
    Ok(Value::Object(response.result))
}

async fn sources(home: &HomeLayout, daemon: &DaemonClient) -> Result<Vec<SessionNodeSource>> {
    let response = daemon
        .call(&DaemonRequest {
            v: 1,
            op: "local_session_list".into(),
            args: json!({"include_offline":true})
                .as_object()
                .expect("list arguments")
                .clone(),
        })
        .await?;
    ensure!(response.ok, "Local native source directory unavailable");
    let sources: Vec<LocalSessionSource> =
        serde_json::from_value(response.result.get("sources").cloned().unwrap_or_default())?;
    let mut exported = Vec::new();
    for source in sources.into_iter().filter(|source| {
        matches!(
            source.runtime,
            cccc_contracts::local_session::LocalSessionRuntime::Codex
                | cccc_contracts::local_session::LocalSessionRuntime::Claude
        ) && !session_nodes::is_remote_source(&source.source_id)
    }) {
        if let Ok(session) = local_sessions::read(home, &source.source_id) {
            exported.push(SessionNodeSource {
                source,
                session_revision: session.revision.clone(),
                alive: session.host_alive(),
            });
        }
    }
    Ok(exported)
}

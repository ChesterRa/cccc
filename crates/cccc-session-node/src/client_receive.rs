use super::protocol::{self, Request};
use anyhow::{Context, Result, ensure};
use cccc_client::DaemonClient;
use cccc_contracts::{
    DaemonRequest,
    local_session::{LocalSessionRuntime, LocalSessionSource},
    session_node::*,
};
use cccc_core::{HomeLayout, local_sessions, session_nodes};
use iroh::endpoint::Connection;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

pub(crate) async fn run_session(
    home: &HomeLayout,
    daemon: &DaemonClient,
    secret: &str,
    connection: &Connection,
    runtimes: Arc<[LocalSessionRuntime]>,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let permits = Arc::new(Semaphore::new(8));
    let mut calls = JoinSet::new();
    // A single bounded rejection task keeps an excess stream off the admission
    // loop. Leave subsequent streams under transport backpressure until a slot opens.
    let mut busy = JoinSet::new();
    let sync_generation = Arc::new(Mutex::new(0));
    let result = async {
        loop {
            tokio::select! {
                _ = connection.closed() => return Ok(()),
                Some(_) = calls.join_next(), if !calls.is_empty() => {},
                Some(_) = busy.join_next(), if !busy.is_empty() => {},
                _ = interval.tick() => {
                    let sources = sources(home, daemon, &runtimes).await?;
                    let response = protocol::call(connection, &Request::Sync { sources }).await?;
                    let mut generation = sync_generation.lock().await;
                    session_nodes::replace_memberships(home, secret, serde_json::from_value(response["memberships"].clone())?)?;
                    *generation += 1;
                }
                incoming = connection.accept_bi(), if permits.available_permits() > 0 || busy.is_empty() => {
                    let (mut send, mut recv) = incoming?;
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                        busy.spawn(async move {
                            // Consume the complete frame before replying. The hub writes
                            // before it reads the response and needs a definite outcome.
                            let response = if matches!(protocol::read(&mut recv).await, Ok(Request::Deliver { .. })) {
                                json!({"outcome":SessionNodeDeliveryOutcome::Unavailable})
                            } else {
                                json!({"error":"Unexpected hub request"})
                            };
                            let _ = protocol::write(&mut send, &response).await;
                        });
                        continue;
                    };
                    let accepted_generation = *sync_generation.lock().await;
                    let sync_generation = Arc::clone(&sync_generation);
                    let runtimes = Arc::clone(&runtimes);
                    let home = home.clone(); let daemon = daemon.clone(); let secret = secret.to_owned();
                    calls.spawn(async move {
                        let _permit = permit;
                        let response = incoming_delivery(&home, &daemon, &secret, &runtimes, protocol::read(&mut recv).await, &sync_generation, accepted_generation).await.unwrap_or_else(|_| json!({"error":"Native handoff authority is unavailable"}));
                        let _ = protocol::write(&mut send, &response).await;
                    });
                }
            }
        }
    }.await;
    // Finish cancelling local stream tasks before the reconnect reuses the secret.
    calls.shutdown().await;
    busy.shutdown().await;
    result
}

pub(super) async fn incoming_delivery(
    home: &HomeLayout,
    daemon: &DaemonClient,
    secret: &str,
    runtimes: &[LocalSessionRuntime],
    request: Result<Request>,
    sync_generation: &Mutex<u64>,
    accepted_generation: u64,
) -> Result<Value> {
    let Request::Deliver { delivery } = request? else {
        anyhow::bail!("Unexpected hub request");
    };
    let client = session_nodes::client(home)?
        .filter(|client| client.secret == secret)
        .context("Connector unavailable")?;
    ensure!(
        delivery.membership.hub_id == client.endpoint.endpoint_id
            && delivery.membership.enabled
            && delivery.source_id == delivery.membership.source_id
            && delivery.source_revision == delivery.membership.source_revision,
        "Unexpected hub membership"
    );
    let source = local_sessions::read(home, &delivery.source_id)?;
    ensure!(
        runtimes.contains(&source.runtime),
        "Runtime unavailable on this connection"
    );
    if source.revision != delivery.source_revision || !source.host_alive() {
        // The hub may still have the old revision until its next source sync.
        // No native submission started, so it can retry with a fresh offer.
        return Ok(json!({"outcome":SessionNodeDeliveryOutcome::Unavailable}));
    }
    {
        let generation = sync_generation.lock().await;
        if *generation == accepted_generation {
            session_nodes::upsert_membership(home, secret, delivery.membership.clone())?;
        } else if !session_nodes::client(home)?.is_some_and(|current| {
            current.secret == secret && current.memberships.contains(&delivery.membership)
        }) {
            // Sync was requested after this stream was accepted. Its snapshot
            // supersedes the earlier offer, and no native submission has started.
            return Ok(json!({"outcome":SessionNodeDeliveryOutcome::Unavailable}));
        }
    }
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

async fn sources(
    home: &HomeLayout,
    daemon: &DaemonClient,
    runtimes: &[LocalSessionRuntime],
) -> Result<Vec<SessionNodeSource>> {
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
        runtimes.contains(&source.runtime) && !session_nodes::is_remote_source(&source.source_id)
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

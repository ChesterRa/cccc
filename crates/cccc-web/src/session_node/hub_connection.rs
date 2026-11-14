use super::{
    hub::Hub,
    protocol::{self, Request},
};
use anyhow::Result;
use cccc_contracts::session_node::SESSION_NODE_PROTOCOL;
use cccc_core::session_nodes;
use iroh::endpoint::Connection;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinSet,
};

impl Hub {
    pub(super) async fn accept(self: Arc<Self>) {
        let permits = Arc::new(Semaphore::new(32));
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { incoming.refuse(); continue; };
                    let hub = Arc::clone(&self);
                    tasks.spawn(async move {
                        let _permit = permit;
                        let connection = match tokio::time::timeout(std::time::Duration::from_secs(10), incoming).await { Ok(Ok(connection)) => connection, _ => return };
                        if let Err(error) = hub.session(connection.clone()).await {
                            // Pairing secrets and raw request bodies are never logged.
                            tracing::debug!(error_class = %error.root_cause(), "LAN session-node connection closed");
                        }
                        let id = connection.remote_id().to_string();
                        let mut connections = hub.connections.lock().await;
                        if connections.get(&id).is_some_and(|(_, current)| current.stable_id() == connection.stable_id()) {
                            connections.remove(&id);
                            let _ = session_nodes::set_connected(&hub.home, &hub.gateway, &id, false);
                        }
                        connection.close(0u32.into(), b"session-node closed");
                    });
                }
            }
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        let _ = session_nodes::clear_endpoint(&self.home, &self.gateway);
    }

    async fn session(self: &Arc<Self>, connection: Connection) -> Result<()> {
        let node_id = connection.remote_id().to_string();
        connection.set_max_concurrent_bi_streams(8u32.into());
        let (mut send, mut recv) =
            tokio::time::timeout(std::time::Duration::from_secs(10), connection.accept_bi())
                .await??;
        let Request::Hello {
            version,
            name,
            invite_id,
            secret,
        } = protocol::read(&mut recv).await?
        else {
            anyhow::bail!("Pairing is required first");
        };
        if version != SESSION_NODE_PROTOCOL {
            protocol::write(&mut send, &json!({"error":"unsupported_protocol"})).await?;
            anyhow::bail!("Unsupported session-node protocol");
        }
        let epoch = uuid::Uuid::new_v4().to_string();
        let mut connections = self.connections.lock().await;
        let peer = match session_nodes::pair(
            &self.home,
            &self.gateway,
            &node_id,
            &name,
            invite_id.as_deref().zip(secret.as_deref()),
        ) {
            Ok(peer) => peer,
            Err(_) => {
                drop(connections);
                protocol::write(&mut send, &json!({"error":"pairing_rejected"})).await?;
                anyhow::bail!("Session-node pairing rejected");
            }
        };
        if let Some((_, old)) =
            connections.insert(node_id.clone(), (epoch.clone(), connection.clone()))
        {
            old.close(0u32.into(), b"replaced by new connector");
        }
        drop(connections);
        protocol::write(
            &mut send,
            &json!({"peer":peer,"memberships":session_nodes::memberships(&self.home, &node_id)?}),
        )
        .await?;
        let receivers = Arc::new(Mutex::new(HashMap::new()));
        let mut calls = JoinSet::new();
        let permits = Arc::new(Semaphore::new(8));
        loop {
            tokio::select! {
                _ = connection.closed() => break,
                Some(_) = calls.join_next(), if !calls.is_empty() => {},
                incoming = connection.accept_bi() => {
                    let Ok((mut send, mut recv)) = incoming else { break; };
                    let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { break; };
                    let hub = Arc::clone(self); let id = node_id.clone(); let epoch = epoch.clone(); let receivers = Arc::clone(&receivers); let conn = connection.clone();
                    calls.spawn(async move {
                        let _permit = permit;
                        let response = match protocol::read(&mut recv).await {
                            Ok(request) => hub.request(&id, &epoch, request, &conn, receivers).await,
                            Err(error) => Err(error),
                        };
                        let response = response.unwrap_or_else(|_| json!({"error":"Remote request is invalid or its authority changed"}));
                        let _ = protocol::write(&mut send, &response).await;
                    });
                }
            }
        }
        calls.abort_all();
        // Each worker sees the closed connection and releases only its own
        // receive lease. Aborting here would strand the old lease on reconnect.
        receivers.lock().await.clear();
        Ok(())
    }

    pub(super) async fn current(&self, node_id: &str, epoch: &str) -> bool {
        self.connections
            .lock()
            .await
            .get(node_id)
            .is_some_and(|(current, _)| current == epoch)
            && session_nodes::peers(&self.home).is_ok_and(|peers| {
                peers
                    .iter()
                    .any(|peer| peer.node_id == node_id && peer.enabled && peer.connected)
            })
    }
}

use super::{hub::Hub, hub_delivery, protocol::Request};
use anyhow::{Context, Result, ensure};
use cccc_contracts::local_session::LocalSessionBinding;
use cccc_core::session_nodes;
use iroh::endpoint::Connection;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

impl Hub {
    pub(super) async fn request(
        self: &Arc<Self>,
        node_id: &str,
        epoch: &str,
        request: Request,
        connection: &Connection,
        receivers: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    ) -> Result<Value> {
        ensure!(
            self.current(node_id, epoch).await,
            "Session-node connection was replaced or revoked"
        );
        match request {
            Request::Sync { sources } => {
                let sources =
                    session_nodes::update_sources(&self.home, &self.gateway, node_id, sources)?;
                hub_delivery::sync_receivers(
                    Arc::clone(self),
                    node_id,
                    epoch,
                    connection,
                    receivers,
                    sources,
                )
                .await;
                Ok(json!({"memberships":session_nodes::memberships(&self.home, node_id)?}))
            }
            Request::Tool {
                source_id,
                revision,
                request,
            } => {
                ensure!(
                    request["method"] == "tools/call",
                    "Only collaboration tool calls are accepted"
                );
                let remote = session_nodes::list_sources(&self.home)?
                    .into_iter()
                    .find(|source| {
                        source.node_id == node_id
                            && source.native_source_id == source_id
                            && source.session_revision == revision
                            && source.alive
                    })
                    .context("Native source is unavailable")?;
                let group_id = request["params"]["arguments"]["group_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .context("Explicit Group ID required")?;
                let membership = session_nodes::memberships(&self.home, node_id)?
                    .into_iter()
                    .find(|membership| {
                        membership.group_id == group_id
                            && membership.source_id == source_id
                            && membership.source_revision == revision
                            && membership.enabled
                    })
                    .context("Remote membership is unavailable")?;
                let binding = LocalSessionBinding {
                    group_id: membership.group_id,
                    actor_id: membership.actor_id,
                    actor_generation: membership.actor_generation,
                    source_id: remote.source.source_id,
                    session_revision: revision,
                };
                tokio::select! { _ = connection.closed() => anyhow::bail!("Connection closed; tool outcome unknown"), result = cccc_mcp::handle_request_for_native_binding(&self.home, &request, &binding) => Ok(result) }
            }
            _ => anyhow::bail!("Unexpected session-node request"),
        }
    }
}

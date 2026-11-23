//! The daemon-side registration of the gateway. Errors become typed IPC codes with safe messages.
use super::{hub, network};
use cccc_contracts::session_node::DEVICE_SESSION_RUNTIMES;
use cccc_core::{HomeLayout, session_nodes};
use cccc_daemon::session_node_host::{HostResult, OpError, SessionNodeHost};
use futures_util::{FutureExt, future::BoxFuture};
use serde_json::{Map, Value, json};

/// Registered by the `cccc` binary before the daemon starts.
pub struct Host;

fn object(value: Value) -> HostResult {
    match value {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

fn failure(error: anyhow::Error) -> OpError {
    if error.downcast_ref::<hub::PortConflict>().is_some() {
        let mut error = OpError::new("session_node_conflict", error.to_string());
        error.details.insert("reason".into(), json!("port_in_use"));
        return error;
    }
    if error.downcast_ref::<hub::RelayUnavailable>().is_some() {
        return OpError::new("session_node_relay_unavailable", error.to_string());
    }
    if let Some(io) = error.downcast_ref::<std::io::Error>()
        && io.kind() == std::io::ErrorKind::WouldBlock
    {
        let mut error = OpError::new("session_node_conflict", io.to_string());
        error
            .details
            .insert("reason".into(), json!("invitation_limit"));
        return error;
    }
    // Only a failed read of the device state file is unreadable state; core's logical refusal for
    // a changed gateway owner is an unavailable gateway.
    if let Some(io) = error.downcast_ref::<std::io::Error>()
        && io
            .get_ref()
            .is_some_and(|cause| cause.is::<session_nodes::StateReadError>())
    {
        return OpError::new("session_node_state_unreadable", io.to_string());
    }
    tracing::warn!(error = %error, "Device gateway operation failed");
    OpError::new(
        "session_node_unavailable",
        "The device gateway could not complete this operation",
    )
}

fn status_of(home: &HomeLayout) -> anyhow::Result<Value> {
    let relay = session_nodes::relay_status(home)?;
    let mut endpoint = session_nodes::endpoint(home)?;
    // Advertise the public origin only while the relay is actually in effect.
    if let Some(endpoint) = &mut endpoint {
        endpoint.relay_url = if relay.effective {
            relay.public_url.clone()
        } else {
            None
        };
    }
    Ok(json!({
        "transport": "iroh",
        "endpoint": endpoint,
        "relay": relay,
        "devices": session_nodes::peers(home)?,
        "supported_runtimes": DEVICE_SESSION_RUNTIMES,
    }))
}

impl SessionNodeHost for Host {
    fn restore(&self, home: HomeLayout) -> BoxFuture<'static, anyhow::Result<()>> {
        async move { hub::restore(&home).await }.boxed()
    }

    fn invite(&self, home: HomeLayout, port: u16) -> BoxFuture<'static, HostResult> {
        async move {
            let ticket = hub::create_invitation(&home, port).await.map_err(failure)?;
            let encoded = network::encode(&ticket).map_err(failure)?;
            object(json!({"ticket": encoded, "expires_at": ticket.expires_at}))
        }
        .boxed()
    }

    fn revoke(&self, home: HomeLayout, node_id: String) -> BoxFuture<'static, HostResult> {
        async move {
            hub::revoke(&home, &node_id).await.map_err(failure)?;
            object(json!({"revoked": true}))
        }
        .boxed()
    }

    fn stop(&self, home: HomeLayout) -> BoxFuture<'static, HostResult> {
        async move {
            hub::stop_explicitly(&home).await.map_err(failure)?;
            object(json!({"stopped": true}))
        }
        .boxed()
    }

    fn status(&self, home: HomeLayout) -> BoxFuture<'static, HostResult> {
        async move {
            // Read under the gateway lock so a first ticket or a stop is seen whole.
            let _hubs = hub::hubs().lock().await;
            object(status_of(&home).map_err(failure)?)
        }
        .boxed()
    }

    fn relay_set(&self, home: HomeLayout, enabled: bool) -> BoxFuture<'static, HostResult> {
        async move {
            hub::set_relay(&home, enabled).await.map_err(failure)?;
            object(json!({"relay": session_nodes::relay_status(&home).map_err(|e| failure(e.into()))?}))
        }
        .boxed()
    }

    fn remote_access_changed(&self, home: HomeLayout) -> BoxFuture<'static, ()> {
        async move { hub::converge_relay(&home).await }.boxed()
    }

    fn shutdown(&self, home: HomeLayout) -> BoxFuture<'static, ()> {
        async move { hub::stop(&home).await }.boxed()
    }
}

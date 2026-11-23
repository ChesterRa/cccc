//! The device-session gateway runs inside the daemon process but is implemented outside this
//! crate (it needs the MCP router, which depends on the daemon). The `cccc` binary registers the
//! implementation before the daemon starts; without one, the management operations report that
//! device sessions are unavailable in this process.
use crate::dispatch::OpResult;
use crate::ops::operation::{Operation, Policy::RemoteAccess};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use futures_util::future::BoxFuture;
use serde_json::{Map, Value};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub use crate::dispatch::OpError;

pub type HostResult = Result<Map<String, Value>, OpError>;

/// Gateway lifecycle owned by the daemon process. Every method runs on the daemon runtime.
pub trait SessionNodeHost: Send + Sync + 'static {
    /// Reopen a previously configured gateway after the daemon listener is ready.
    fn restore(&self, home: HomeLayout) -> BoxFuture<'static, anyhow::Result<()>>;
    fn invite(&self, home: HomeLayout, port: u16) -> BoxFuture<'static, HostResult>;
    fn revoke(&self, home: HomeLayout, node_id: String) -> BoxFuture<'static, HostResult>;
    fn stop(&self, home: HomeLayout) -> BoxFuture<'static, HostResult>;
    fn status(&self, home: HomeLayout) -> BoxFuture<'static, HostResult>;
    fn relay_set(&self, home: HomeLayout, enabled: bool) -> BoxFuture<'static, HostResult>;
    /// Bring a live gateway's loopback relay in line with changed Remote Access settings.
    fn remote_access_changed(&self, home: HomeLayout) -> BoxFuture<'static, ()>;
    /// Release process-owned resources on daemon shutdown without recording an explicit stop.
    fn shutdown(&self, home: HomeLayout) -> BoxFuture<'static, ()>;
}

static HOST: OnceLock<Arc<dyn SessionNodeHost>> = OnceLock::new();

/// Register the gateway implementation for this process. Later registrations are ignored.
pub fn register(host: Arc<dyn SessionNodeHost>) {
    let _ = HOST.set(host);
}

fn host() -> Option<Arc<dyn SessionNodeHost>> {
    HOST.get().cloned()
}

const SHUTDOWN_LIMIT: Duration = Duration::from_secs(5);

pub(crate) fn spawn_restore(home: &HomeLayout) {
    let Some(host) = host() else { return };
    let home = home.clone();
    tokio::spawn(async move {
        if let Err(error) = host.restore(home).await {
            tracing::warn!(%error, "Could not restore the device-session gateway");
        }
    });
}

/// Remote Access settings decide whether the loopback relay may run. Every daemon writer of
/// those settings calls this after saving them; the gateway converges without blocking it.
pub(crate) fn remote_access_changed(home: &HomeLayout) {
    let (Some(host), Ok(runtime)) = (host(), tokio::runtime::Handle::try_current()) else {
        return;
    };
    runtime.spawn(host.remote_access_changed(home.clone()));
}

pub(crate) async fn shutdown(home: &HomeLayout) {
    let Some(host) = host() else { return };
    if tokio::time::timeout(SHUTDOWN_LIMIT, host.shutdown(home.clone()))
        .await
        .is_err()
    {
        tracing::warn!("Device-session gateway shutdown timed out");
    }
}

pub(crate) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        // Gateway management shares the Remote Access permit: binding may wait for a port and
        // relay changes read Remote Access settings, but neither may hold Group work back.
        "session_node_invite" => Operation::new(RemoteAccess, invite),
        "session_node_revoke" => Operation::new(RemoteAccess, revoke),
        "session_node_stop" => Operation::new(RemoteAccess, stop),
        // Status waits for the gateway lock (a restore may hold it while a saved port is
        // still being released), so it must not hold a Group read permit meanwhile.
        "session_node_status" => Operation::new(RemoteAccess, status),
        "session_node_relay_set" => Operation::new(RemoteAccess, relay_set),
        _ => return None,
    })
}

/// Management stays with the local administrator: the trusted CLI or an authenticated
/// administrator Web route, never a native, cloud or device-bound caller.
fn administrator(request: &DaemonRequest) -> Result<(), OpError> {
    let bound = [
        "_cccc_web_binding",
        "_cccc_native_binding",
        "_cccc_native_source_binding",
    ]
    .iter()
    .any(|key| request.args.contains_key(*key));
    if bound || request.args.get("by").and_then(Value::as_str) != Some("user") {
        return Err(OpError::new(
            "permission_denied",
            "Device-session management is limited to the local administrator",
        ));
    }
    Ok(())
}

fn run(
    request: &DaemonRequest,
    call: impl FnOnce(Arc<dyn SessionNodeHost>) -> BoxFuture<'static, HostResult>,
) -> OpResult {
    administrator(request)?;
    let host = host().ok_or_else(|| {
        OpError::new(
            "session_node_unavailable",
            "Device sessions are not available in this daemon process",
        )
    })?;
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
        OpError::new(
            "session_node_unavailable",
            "Device sessions need the daemon runtime",
        )
    })?;
    runtime.block_on(call(host))
}

/// A daemon that is shutting down opens nothing new.
fn not_stopping(home: &HomeLayout) -> Result<(), OpError> {
    if crate::session_owner::selected_shutdown(home)
        .map_err(OpError::io)?
        .is_some()
    {
        return Err(OpError::new(
            "shutdown_in_progress",
            "The CCCC daemon is shutting down",
        ));
    }
    Ok(())
}

fn invite(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let port = match request.args.get("port") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .ok_or_else(|| {
                OpError::new("invalid_args", "port must be an integer from 0 to 65535")
            })?,
    };
    administrator(request)?;
    not_stopping(home)?;
    let home = home.clone();
    run(request, move |host| host.invite(home, port))
}

fn revoke(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let node_id = request
        .args
        .get("node_id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .ok_or_else(|| OpError::new("invalid_args", "node_id must be a device identifier"))?
        .to_owned();
    let home = home.clone();
    run(request, move |host| host.revoke(home, node_id))
}

fn stop(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let home = home.clone();
    run(request, move |host| host.stop(home))
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let home = home.clone();
    run(request, move |host| host.status(home))
}

fn relay_set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let enabled = request
        .args
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| OpError::new("invalid_args", "enabled must be a boolean"))?;
    if enabled {
        administrator(request)?;
        not_stopping(home)?;
    }
    let home = home.clone();
    run(request, move |host| host.relay_set(home, enabled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::operation::Policy;
    use serde_json::json;

    fn request(op: &str, args: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        }
    }

    fn error(op: &str, args: Value) -> String {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = request(op, args);
        let operation = resolve_operation(&request).expect("device-session operation");
        operation
            .execute(&home, &request)
            .expect_err("refused")
            .code
    }

    #[test]
    fn management_stays_with_the_local_administrator() {
        let ops = [
            "session_node_invite",
            "session_node_revoke",
            "session_node_stop",
            "session_node_status",
            "session_node_relay_set",
        ];
        for op in ops {
            let args = json!({"node_id":"abc","enabled":true});
            let mut member = args.clone();
            member["by"] = json!("peer-1");
            assert_eq!(error(op, member), "permission_denied", "{op} by a member");
            for binding in [
                "_cccc_web_binding",
                "_cccc_native_binding",
                "_cccc_native_source_binding",
            ] {
                let mut bound = args.clone();
                bound["by"] = json!("user");
                bound[binding] = json!({});
                assert_eq!(error(op, bound), "permission_denied", "{op} with {binding}");
            }
            // The administrator reaches the gateway, which this test process never registers.
            let mut user = args.clone();
            user["by"] = json!("user");
            assert_eq!(
                error(op, user),
                "session_node_unavailable",
                "{op} by the administrator"
            );
        }
        assert_eq!(
            error(
                "session_node_revoke",
                json!({"by":"user","node_id":"../peer"})
            ),
            "invalid_args"
        );
        assert_eq!(
            error("session_node_invite", json!({"by":"user","port":70000})),
            "invalid_args"
        );
    }

    #[test]
    fn management_does_not_hold_group_work() {
        for op in [
            "session_node_invite",
            "session_node_revoke",
            "session_node_stop",
            "session_node_status",
            "session_node_relay_set",
        ] {
            let policy = resolve_operation(&request(op, json!({})))
                .expect("operation")
                .policy;
            assert!(matches!(policy, Policy::RemoteAccess), "{op}");
        }
    }
}

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::io;

use crate::ops;
use crate::ops::operation::{Operation, Policy};

pub type OpResult = Result<Map<String, Value>, OpError>;

pub fn dispatch(home: &HomeLayout, request: &DaemonRequest) -> DaemonResponse {
    match dispatch_result(home, request) {
        Ok(result) => DaemonResponse::success(result),
        Err(error) => {
            let mut response = DaemonResponse::failure(error.code, error.message);
            if let Some(body) = response.error.as_mut() {
                body.details = error.details;
            }
            response
        }
    }
}

fn dispatch_result(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    resolve_operation(request)
        .ok_or_else(|| OpError::new("unknown_op", format!("unknown operation: {}", request.op)))?
        .execute(home, request)
}

pub(crate) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "ping" => Operation::new(Policy::Read, ping),
        "version" => Operation::new(Policy::Read, |_home, _request| {
            object(
                json!({"version": env!("CARGO_PKG_VERSION"), "implementation": "rust", "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY}),
            )
        }),
        "home_get" => Operation::new(Policy::Read, |home, _request| {
            object(json!({"home": home.root(), "environment": "CCCC_HOME"}))
        }),
        "shutdown" => Operation::new(Policy::GlobalWrite, |_home, request| shutdown(request)),
        _ => return ops::resolve_operation(request),
    })
}

fn ping(_home: &HomeLayout, _request: &DaemonRequest) -> OpResult {
    object(json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "ts": cccc_contracts::utc_now(),
        "ipc_v": 1,
        "capabilities": {
            "events_stream": true,
            "remote_access": true,
            "presentation_browser_attach": false,
            "presentation_browser_vnc_attach": false,
            "space_provider_auth_browser_attach": false,
            "space_provider_auth_browser_vnc_attach": false,
            "web_model_browser_attach": false,
            "web_model_browser_vnc_attach": false,
            "term_attachment_status": true,
            "term_attach_snapshot_v1": true,
            "assistant_state": true,
            "assistant_voice_recording_lease": true,
            "assistant_voice_model_install": false,
        },
        "implementation": "rust",
        "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY,
    }))
}

fn shutdown(request: &DaemonRequest) -> OpResult {
    if let Some(value) = request.args.get("expected_pid") {
        let expected_pid = value.as_u64().filter(|pid| *pid > 0).ok_or_else(|| {
            OpError::new("invalid_args", "expected_pid must be a positive integer")
        })?;
        let current_pid = u64::from(std::process::id());
        if expected_pid != current_pid {
            return Err(OpError::new(
                "daemon_owner_mismatch",
                format!(
                    "shutdown expected daemon pid {expected_pid}, but connected daemon pid is {current_pid}"
                ),
            ));
        }
    }
    object(json!({"shutting_down": true}))
}

pub fn required_arg(request: &DaemonRequest, name: &str) -> Result<String, OpError> {
    string_arg(request, name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OpError::new("invalid_args", format!("{name} is required")))
}

pub fn string_arg(request: &DaemonRequest, name: &str) -> Option<String> {
    request
        .args
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn first_non_blank_arg(request: &DaemonRequest, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        string_arg(request, name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

pub fn bool_arg(request: &DaemonRequest, name: &str, default: bool) -> bool {
    request
        .args
        .get(name)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

pub fn object<T: Serialize>(value: T) -> OpResult {
    value_map(serde_json::to_value(value).map_err(OpError::invalid)?)
}

pub fn value_map(value: Value) -> OpResult {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| OpError::new("internal_error", "result is not an object"))
}

pub fn store(home: &HomeLayout) -> Result<GroupStore, OpError> {
    GroupStore::new(home.clone()).map_err(OpError::io)
}

#[derive(Debug)]
pub struct OpError {
    pub code: String,
    pub message: String,
    pub details: Map<String, Value>,
}

impl OpError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Map::new(),
        }
    }
    pub fn io(error: io::Error) -> Self {
        Self::new("io_error", error.to_string())
    }
    pub fn not_found(error: io::Error) -> Self {
        Self::new("not_found", error.to_string())
    }
    pub fn invalid(error: impl std::fmt::Display) -> Self {
        Self::new("invalid_args", error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, first_non_blank_arg};
    use cccc_contracts::DaemonRequest;
    use cccc_core::HomeLayout;
    use serde_json::json;

    #[test]
    fn documented_operations_resolve_or_explicitly_advertise_unavailability() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Default::default(),
        };
        let ping = dispatch(&home, &request);
        let spec = include_str!("../../../docs/standards/CCCC_DAEMON_IPC_V1.md");
        for line in spec.lines().filter(|line| line.starts_with("#### ")) {
            for op in line.split('`').skip(1).step_by(2).filter(|op| {
                !op.is_empty()
                    && op
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            }) {
                // These change the connection protocol and are handled before
                // ordinary dispatch in server_connection (with stream regressions).
                if matches!(op, "term_attach" | "events_stream") {
                    continue;
                }
                let request = DaemonRequest {
                    v: 1,
                    op: op.into(),
                    args: Default::default(),
                };
                let available = super::resolve_operation(&request).is_some();
                if ping.result["capabilities"]
                    .get(op)
                    .and_then(serde_json::Value::as_bool)
                    == Some(false)
                {
                    assert!(
                        !available,
                        "{op} has a handler but advertises unavailability"
                    );
                } else {
                    assert!(
                        available,
                        "documented operation {op} has no executable route"
                    );
                }
            }
        }
    }

    #[test]
    fn string_aliases_skip_empty_primary_values() {
        let request = DaemonRequest {
            v: 1,
            op: "test".into(),
            args: json!({"primary":"  ","legacy":" value "})
                .as_object()
                .cloned()
                .expect("args"),
        };

        assert_eq!(
            first_non_blank_arg(&request, &["primary", "legacy"]).as_deref(),
            Some("value")
        );
    }

    #[test]
    fn shutdown_rejects_a_different_daemon_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":u64::from(std::process::id()) + 1})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let response = dispatch(&home, &request);
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("owner mismatch").code,
            "daemon_owner_mismatch"
        );
    }

    #[test]
    fn shutdown_accepts_its_own_daemon_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":std::process::id()})
                .as_object()
                .cloned()
                .expect("args"),
        };

        assert!(dispatch(&home, &request).ok);
    }

    #[test]
    fn shutdown_rejects_an_invalid_expected_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":0})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let response = dispatch(&home, &request);
        assert!(!response.ok);
        assert_eq!(response.error.expect("invalid pid").code, "invalid_args");
    }
}

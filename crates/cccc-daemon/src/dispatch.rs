use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::io;

use crate::ops;

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
    let core = match request.op.as_str() {
        "ping" => Some(object(json!({
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "ts": cccc_contracts::utc_now(),
            "ipc_v": 1,
            "capabilities": {
                "events_stream": true,
                "remote_access": true,
            },
            "implementation": "rust",
            "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY,
        }))),
        "version" => Some(object(
            json!({"version": env!("CARGO_PKG_VERSION"), "implementation": "rust", "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY}),
        )),
        "home_get" => Some(object(
            json!({"home": home.root(), "environment": "CCCC_HOME"}),
        )),
        "shutdown" => Some(object(json!({"shutting_down": true}))),
        _ => None,
    };
    if let Some(result) = core {
        return result;
    }
    ops::handle(home, request)?
        .ok_or_else(|| OpError::new("unknown_op", format!("unknown operation: {}", request.op)))?
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
    use super::first_non_blank_arg;
    use cccc_contracts::DaemonRequest;
    use serde_json::json;

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
}

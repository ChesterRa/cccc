use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::group_bridge_store::{BridgeStore, items};
use crate::AppState;
use crate::api::ApiError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccessGrant {
    pub(super) level: String,
    trust_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionBinding {
    registration_id: String,
    trust_id: String,
    group_id: String,
    remote_group_id: String,
}

pub(super) fn access_grant(
    state: &AppState,
    registration: &Value,
) -> Result<AccessGrant, ApiError> {
    let trust = items(
        &BridgeStore::new(&state.home)
            .load()
            .map_err(|error| ApiError::bad(error.to_string()))?,
        "trusts",
    )
    .iter()
    .find(|item| {
        item["registration_id"] == registration["registration_id"] && item["status"] == "active"
    })
    .cloned();
    Ok(AccessGrant {
        level: trust
            .as_ref()
            .and_then(|item| item["access_level"].as_str())
            .unwrap_or("messages")
            .to_owned(),
        trust_id: trust
            .as_ref()
            .and_then(|item| item["trust_id"].as_str())
            .unwrap_or("")
            .to_owned(),
    })
}

pub(super) fn require(
    arguments: &Map<String, Value>,
    registration: &Value,
    grant: &AccessGrant,
) -> Result<(), ApiError> {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let expected = binding(registration, grant);
    let matches = bindings()
        .lock()
        .map_err(lock_error)?
        .get(session_id)
        .is_some_and(|stored| stored == &expected);
    if matches {
        return Ok(());
    }
    Err(ApiError::bad_code(
        "bridge_session_not_found",
        "remote exec session is not bound to this active bridge",
        json!({"session_id":session_id}),
    ))
}

pub(super) fn update(
    tool_name: &str,
    registration: &Value,
    grant: &AccessGrant,
    response: &Value,
) -> Result<(), ApiError> {
    let Some(session_id) = response
        .pointer("/result/structuredContent/session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let mut bindings = bindings().lock().map_err(lock_error)?;
    if tool_name == "cccc_remote_exec_command" {
        bindings.insert(session_id.to_owned(), binding(registration, grant));
    } else if tool_name == "cccc_remote_write_stdin"
        && response
            .pointer("/result/structuredContent/status/running")
            .and_then(Value::as_bool)
            == Some(false)
    {
        bindings.remove(session_id);
    }
    Ok(())
}

fn binding(registration: &Value, grant: &AccessGrant) -> SessionBinding {
    SessionBinding {
        registration_id: registration["registration_id"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
        trust_id: grant.trust_id.clone(),
        group_id: registration["group_id"].as_str().unwrap_or("").to_owned(),
        remote_group_id: registration["remote_group_id"]
            .as_str()
            .unwrap_or("")
            .to_owned(),
    }
}

fn bindings() -> &'static Mutex<HashMap<String, SessionBinding>> {
    static BINDINGS: OnceLock<Mutex<HashMap<String, SessionBinding>>> = OnceLock::new();
    BINDINGS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> ApiError {
    ApiError::unavailable("bridge_session_store_error", "session lock poisoned")
}

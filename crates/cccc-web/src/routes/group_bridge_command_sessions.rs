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
        item["registration_id"] == registration["registration_id"]
            && item["group_id"] == registration["group_id"]
            && item["status"] == "active"
    })
    .cloned()
    .ok_or_else(|| ApiError::forbidden("group bridge trust is not active"))?;
    Ok(AccessGrant {
        level: trust["access_level"]
            .as_str()
            .unwrap_or("messages")
            .to_owned(),
        trust_id: trust["trust_id"].as_str().unwrap_or("").to_owned(),
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
    requested_session_id: Option<&str>,
    terminate_requested: bool,
) -> Result<(), ApiError> {
    let response_session_id = response
        .pointer("/result/structuredContent/session_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let mut bindings = bindings().lock().map_err(lock_error)?;
    if tool_name == "cccc_remote_exec_command"
        && let Some(session_id) = response_session_id
    {
        bindings.insert(session_id.to_owned(), binding(registration, grant));
    } else if tool_name == "cccc_remote_write_stdin"
        && (terminate_requested
            || response
                .pointer("/result/structuredContent/status/running")
                .and_then(Value::as_bool)
                == Some(false))
        && let Some(session_id) = response_session_id.or(requested_session_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminate_request_revokes_binding_without_running_status() {
        let session_id = format!("s_{}", uuid::Uuid::new_v4().simple());
        let registration = registration();
        let grant = grant();
        let started = json!({
            "result":{"structuredContent":{"session_id":session_id}}
        });
        update(
            "cccc_remote_exec_command",
            &registration,
            &grant,
            &started,
            None,
            false,
        )
        .expect("bind session");

        let arguments = json!({"session_id":session_id})
            .as_object()
            .expect("arguments")
            .clone();
        require(&arguments, &registration, &grant).expect("binding exists");

        let terminated = json!({
            "jsonrpc":"2.0",
            "error":{"code":-32602,"message":"runtime session not found"}
        });
        update(
            "cccc_remote_write_stdin",
            &registration,
            &grant,
            &terminated,
            Some(&session_id),
            true,
        )
        .expect("revoke binding");

        assert!(
            require(&arguments, &registration, &grant).is_err(),
            "binding should be removed"
        );
    }

    fn registration() -> Value {
        json!({
            "registration_id":"greg_test",
            "group_id":"g_target",
            "remote_group_id":"g_source"
        })
    }

    fn grant() -> AccessGrant {
        AccessGrant {
            level: "full".into(),
            trust_id: "trust_test".into(),
        }
    }
}

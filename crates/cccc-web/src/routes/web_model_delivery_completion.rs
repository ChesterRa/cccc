use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::ApiError;

use super::web_model_delivery_state::{record_connector, update_target};

const MAX_RECONCILE_ATTEMPTS: u64 = 3;

pub(super) async fn call(
    state: &AppState,
    op: &str,
    args: Map<String, Value>,
) -> Result<Value, ApiError> {
    raw_call(state, op, args).await.map_err(|error| error.api)
}

pub(super) fn args(group_id: &str, actor_id: &str) -> Map<String, Value> {
    json!({"group_id":group_id,"actor_id":actor_id})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

pub(super) fn complete_args(
    group_id: &str,
    actor_id: &str,
    turn_id: &str,
    event_ids: Value,
    delivery_id: &str,
) -> Map<String, Value> {
    json!({
        "group_id":group_id,
        "actor_id":actor_id,
        "turn_id":turn_id,
        "event_ids":event_ids,
        "delivery_id":delivery_id,
        "status":"done",
        "by":actor_id
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

pub(super) async fn reconcile(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    target: &Value,
) -> Result<bool, ApiError> {
    let Some(evidence) = Evidence::from_target(target) else {
        return Ok(false);
    };
    if evidence.attempts >= MAX_RECONCILE_ATTEMPTS {
        return Ok(false);
    }
    let request = complete_args(
        group_id,
        actor_id,
        &evidence.turn_id,
        evidence.event_ids,
        &evidence.delivery_id,
    );
    match raw_call(state, "web_model_runtime_complete_turn", request).await {
        Ok(_) => {
            let submission_ambiguous =
                target["last_delivery_status"] == "submission_ambiguous_completion_pending";
            let pending_new_chat_bind = target["kind"] == "new_chat";
            let final_status = if submission_ambiguous {
                "submission_ambiguous"
            } else if pending_new_chat_bind {
                "pending_new_chat_bind"
            } else {
                "submitted"
            };
            let final_error = if submission_ambiguous {
                "browser submission was attempted but could not be verified; this message will not be redelivered automatically"
            } else if pending_new_chat_bind {
                "conversation_url_pending"
            } else {
                ""
            };
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":final_status,
                    "last_delivery_reconciled_at":cccc_contracts::utc_now(),
                    "last_error":final_error
                }),
            )?;
            record_connector(
                state,
                group_id,
                actor_id,
                if submission_ambiguous {
                    "ambiguous"
                } else {
                    "submitted"
                },
                &evidence.turn_id,
                final_error,
            )?;
            Ok(true)
        }
        Err(error) => {
            let attempts = evidence.attempts + 1;
            let conflict = error.code.as_deref() == Some("completion_conflict");
            let submission_ambiguous =
                target["last_delivery_status"] == "submission_ambiguous_completion_pending";
            update_target(
                state,
                group_id,
                actor_id,
                json!({
                    "last_delivery_status":if conflict {
                        "completion_conflict"
                    } else if submission_ambiguous {
                        "submission_ambiguous_completion_pending"
                    } else {
                        "ambiguous"
                    },
                    "last_delivery_reconcile_attempts":if conflict {MAX_RECONCILE_ATTEMPTS} else {attempts},
                    "last_error":error.api.to_string()
                }),
            )?;
            if conflict {
                record_connector(
                    state,
                    group_id,
                    actor_id,
                    "failed",
                    &evidence.turn_id,
                    &error.api.to_string(),
                )?;
            }
            Ok(false)
        }
    }
}

struct Evidence {
    turn_id: String,
    event_ids: Value,
    delivery_id: String,
    attempts: u64,
}

impl Evidence {
    fn from_target(target: &Value) -> Option<Self> {
        let turn_id = nonempty(target, "last_delivery_turn_id")?;
        let delivery_id = nonempty(target, "last_delivery_id")?;
        let event_ids = target["last_delivery_event_ids"].as_array()?;
        if event_ids.is_empty() || event_ids.iter().any(|value| !value.is_string()) {
            return None;
        }
        Some(Self {
            turn_id,
            event_ids: Value::Array(event_ids.clone()),
            delivery_id,
            attempts: target["last_delivery_reconcile_attempts"]
                .as_u64()
                .unwrap_or(0),
        })
    }
}

struct CallError {
    code: Option<String>,
    api: ApiError,
}

async fn raw_call(
    state: &AppState,
    op: &str,
    args: Map<String, Value>,
) -> Result<Value, CallError> {
    let response = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| CallError {
            code: None,
            api: ApiError::unavailable("daemon_unavailable", error.to_string()),
        })?;
    if response.ok {
        return Ok(Value::Object(response.result));
    }
    let (code, message) = response.error.map_or_else(
        || ("daemon_error".into(), "daemon operation failed".into()),
        |error| (error.code, error.message),
    );
    Err(CallError {
        code: Some(code.clone()),
        api: ApiError::bad_code(code, message, json!({})),
    })
}

fn nonempty(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

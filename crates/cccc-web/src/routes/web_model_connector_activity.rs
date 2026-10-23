use serde_json::{Value, json};

use crate::AppState;
use crate::api::ApiError;

use super::web_model_connector_store;

pub(super) struct Activity<'a> {
    pub method: &'a str,
    pub tool_name: &'a str,
    pub call_status: &'a str,
    pub wait_status: &'a str,
    pub turn_id: &'a str,
    pub error: &'a str,
}

pub(super) fn record(
    state: &AppState,
    connector_id: &str,
    activity: Activity<'_>,
) -> Result<(), ApiError> {
    record_route(state, connector_id, None, activity, None)
}

pub(super) fn record_route(
    state: &AppState,
    connector_id: &str,
    route: Option<&Value>,
    activity: Activity<'_>,
    host_identity: Option<&Value>,
) -> Result<(), ApiError> {
    web_model_connector_store::update_connector(state, connector_id, |connector| {
        let item = if let Some(route) = route {
            let key = json!([route["group_id"], route["actor_id"]]).to_string();
            let Some(item) = connector["bindings"].get_mut(key) else {
                return;
            };
            if item["revision"] != route["revision"] {
                return;
            }
            item
        } else {
            &mut *connector
        };

        item["last_activity_at"] = json!(cccc_contracts::utc_now());
        item["last_method"] = json!(activity.method);
        item["last_tool_name"] = json!(activity.tool_name);
        item["last_call_status"] = json!(activity.call_status);
        item["last_wait_status"] = json!(activity.wait_status);
        item["last_turn_id"] = json!(activity.turn_id);
        item["last_error"] = json!(activity.error);
        if route.is_none() {
            item["updated_at"] = json!(cccc_contracts::utc_now());
        }
        if retain_tool_call(&activity) {
            let mut entry = json!({"at":cccc_contracts::utc_now(),"method":"tools/call",
                "tool_name":activity.tool_name,"ok":activity.call_status == "ok",
                "reason":(!activity.error.is_empty()).then_some(activity.error)});
            if let Some(identity) = host_identity {
                entry["host_identity"] = identity.clone();
            }
            append_call(connector, entry);
        }
    })
    .map(|_| ())
}

/// Shared, bounded diagnostics; observations do not displace actionable calls.
pub(super) fn append_call(connector: &mut Value, entry: Value) {
    let mut log = connector["events_log"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    log.push(entry);
    let excess = log.len().saturating_sub(10);
    log.drain(..excess);
    connector["events_log"] = Value::Array(log);
}

fn retain_tool_call(activity: &Activity<'_>) -> bool {
    activity.method == "tools/call"
        && (matches!(
            activity.tool_name,
            "cccc_pair" | "cccc_runtime_wait_next_turn" | "cccc_runtime_complete_turn"
        ) || (activity.call_status == "error"
            && cccc_mcp::web_model_catalog()
                .iter()
                .any(|tool| tool["name"] == activity.tool_name)))
}

pub(super) fn details(tool_name: &str, response: &Value) -> (String, String, String) {
    let text = response["result"]["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .unwrap_or("");
    let parsed = response["result"]
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| serde_json::from_str::<Value>(text).unwrap_or(Value::Null));
    let error = if response.get("error").is_some()
        || response["result"]["isError"].as_bool().unwrap_or(false)
    {
        safe_reason(response.get("error").unwrap_or(&parsed["error"]))
    } else {
        String::new()
    };
    let wait_status = if matches!(
        tool_name,
        "cccc_runtime_wait_next_turn" | "cccc_runtime_complete_turn"
    ) {
        parsed["status"].as_str().unwrap_or("").to_owned()
    } else {
        String::new()
    };
    let turn_id = parsed["turn"]["turn_id"]
        .as_str()
        .or_else(|| parsed["turn_id"].as_str())
        .unwrap_or("")
        .to_owned();
    (wait_status, turn_id, error)
}

/// Errors can contain private arguments. Keep known codes, never raw messages or data.
fn safe_reason(error: &Value) -> String {
    for text in [
        error.as_str(),
        error["message"].as_str(),
        error["code"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        // Prefer the specific inner reason over wrappers such as "io_error:".
        for segment in text.rsplit(':') {
            let code = segment.split_whitespace().next().unwrap_or("");
            if matches!(
                code,
                "pairing_expired"
                    | "pairing_invalid"
                    | "pairing_changed"
                    | "pairing_failed"
                    | "pairing_already_used"
                    | "pairing_not_received"
                    | "conversation_not_paired"
                    | "conversation_pairing_changed"
                    | "paired_actor_stopped"
                    | "paired_actor_identity_changed"
                    | "paired_actor_runtime_changed"
                    | "paired_actor_unavailable"
                    | "session_missing"
                    | "session_metadata_invalid"
                    | "binding_conflict"
                    | "dots_binding_conflict"
                    | "dots_binding_changed"
                    | "dots_binding_unavailable"
                    | "dots_pairing_changed"
                    | "actor_token_invalid"
                    | "dots_token_invalid"
                    | "dots_execution_token_required"
                    | "dots_task_expired"
                    | "dots_task_required"
                    | "dots_actor_stopped"
                    | "dots_completion_conflict"
                    | "dots_runtime_state_unavailable"
                    | "actor_must_be_stopped"
                    | "permission_denied"
                    | "invalid_argument"
                    | "invalid_args"
                    | "invalid_request"
                    | "invalid_action"
                    | "not_found"
                    | "actor_not_found"
                    | "io_error"
            ) {
                return code.to_owned();
            }
        }
    }
    match error["code"].as_i64() {
        Some(-32700 | -32600 | -32601 | -32602 | -32603) => format!("rpc_{}", error["code"]),
        _ => "tool_call_failed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_fixed_reasons_across_mcp_result_shapes() {
        let failures = [
            json!({"result":{"isError":true,"structuredContent":{"error":"io_error: pairing_expired"}}}),
            json!({"result":{"isError":true,"content":[{"type":"text","text":r#"{"error":{"code":"pairing_changed","message":"private synthetic value"}}"#}]}}),
            json!({"error":{"code":-32602,"message":"private synthetic value","data":{"actor_token":"synthetic secret"}}}),
            json!({"result":{"isError":true,"structuredContent":{"error":"private synthetic value"}}}),
            json!({"result":{"isError":true,"structuredContent":{"error":"permission_denied: dots_pairing_changed: get new setup instructions"}}}),
        ];
        for (response, expected) in failures.iter().zip([
            "pairing_expired",
            "pairing_changed",
            "rpc_-32602",
            "tool_call_failed",
            "dots_pairing_changed",
        ]) {
            assert_eq!(details("cccc_pair", response).2, expected);
        }
    }

    #[test]
    fn successful_user_data_is_not_a_failed_call_and_claim_metadata_survives() {
        let response = json!({"result":{"structuredContent":{"status":"work_available",
            "turn":{"turn_id":"turn_fixture"},"error":"pairing_expired"}}});
        assert_eq!(
            details("cccc_runtime_wait_next_turn", &response),
            (
                "work_available".into(),
                "turn_fixture".into(),
                String::new()
            )
        );
    }

    #[test]
    fn history_is_bounded_and_observations_do_not_displace_it() {
        let mut connector = json!({});
        for index in 0..12 {
            append_call(
                &mut connector,
                json!({"method":"events/subscribe","ok":false,"reason":"challenge_failed","index":index}),
            );
        }
        assert_eq!(
            connector["events_log"]
                .as_array()
                .expect("call history")
                .len(),
            10
        );
        assert_eq!(connector["events_log"][0]["index"], 2);
        let mut activity = Activity {
            method: "tools/call",
            tool_name: "cccc_connector_status",
            call_status: "ok",
            wait_status: "",
            turn_id: "",
            error: "",
        };
        assert!(!retain_tool_call(&activity));
        activity.tool_name = "cccc_pair";
        assert!(retain_tool_call(&activity));
        activity.tool_name = "cccc_message_send";
        activity.call_status = "error";
        assert!(retain_tool_call(&activity));
        activity.tool_name = "private synthetic tool name";
        assert!(!retain_tool_call(&activity));
    }
}

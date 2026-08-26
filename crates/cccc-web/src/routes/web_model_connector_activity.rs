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
    web_model_connector_store::update_connector(state, connector_id, |item| {
        item["last_activity_at"] = json!(cccc_contracts::utc_now());
        item["last_method"] = json!(activity.method);
        item["last_tool_name"] = json!(activity.tool_name);
        item["last_call_status"] = json!(activity.call_status);
        item["last_wait_status"] = json!(activity.wait_status);
        item["last_turn_id"] = json!(activity.turn_id);
        item["last_error"] = json!(activity.error);
        item["updated_at"] = json!(cccc_contracts::utc_now());
    })
    .map(|_| ())
}

pub(super) fn details(tool_name: &str, response: &Value) -> (String, String, String) {
    let protocol_error = response
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let text = response["result"]["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .unwrap_or("");
    let parsed = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let error = parsed["error"]["message"]
        .as_str()
        .filter(|message| !message.is_empty())
        .unwrap_or(protocol_error)
        .to_owned();
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

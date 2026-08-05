use cccc_core::integration_state;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::ApiError;

use super::web_model_connector_store::{STORE_KEY, ensure_array, io_error};

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
    integration_state::global_update(&state.home, STORE_KEY, |value| {
        let Some(item) = ensure_array(value)
            .iter_mut()
            .find(|item| item["connector_id"] == connector_id)
        else {
            return Ok(());
        };
        item["last_activity_at"] = json!(cccc_contracts::utc_now());
        item["last_method"] = json!(activity.method);
        item["last_tool_name"] = json!(activity.tool_name);
        item["last_call_status"] = json!(activity.call_status);
        item["last_wait_status"] = json!(activity.wait_status);
        item["last_turn_id"] = json!(activity.turn_id);
        item["last_error"] = json!(activity.error);
        item["updated_at"] = json!(cccc_contracts::utc_now());
        Ok(())
    })
    .map_err(io_error)
}

pub(super) fn details(tool_name: &str, response: &Value) -> (String, String, String) {
    let error = response
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let text = response["result"]["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .unwrap_or("");
    let parsed = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
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

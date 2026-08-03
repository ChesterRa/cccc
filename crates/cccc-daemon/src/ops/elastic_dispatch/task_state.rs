use cccc_contracts::DaemonRequest;
use cccc_core::context::ContextDoc;
use serde_json::{Map, Value};

use crate::dispatch::string_arg;

pub(super) fn has_active_task(context: &ContextDoc, actor_id: &str) -> bool {
    context.tasks.iter().any(|task| {
        !terminal_task(task)
            && ["assignee", "handoff_to"]
                .into_iter()
                .any(|key| task.get(key).and_then(Value::as_str) == Some(actor_id))
    })
}

pub(super) fn terminal_task(task: &Map<String, Value>) -> bool {
    matches!(
        task.get("status")
            .and_then(Value::as_str)
            .unwrap_or("planned")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "done" | "archived"
    )
}

pub(super) fn existing_idempotent_assignee(
    context: &ContextDoc,
    group_id: &str,
    by: &str,
    request: &DaemonRequest,
) -> Option<String> {
    let key = string_arg(request, "idempotency_key")
        .or_else(|| string_arg(request, "client_request_id"))?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let client_id = super::super::message_idempotency::tracked_client_id(group_id, by, key);
    context.tasks.iter().rev().find_map(|task| {
        (task.get("client_request_id").and_then(Value::as_str) == Some(client_id.as_str()))
            .then(|| {
                task.get("assignee")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten()
    })
}

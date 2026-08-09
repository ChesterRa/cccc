use cccc_contracts::Event;
use serde_json::{Map, Value, json};

use crate::GroupDoc;

pub fn notify_event(
    group: &GroupDoc,
    rule_id: &str,
    rule: &Map<String, Value>,
    action: Option<&Map<String, Value>>,
) -> Option<Event> {
    let message = action
        .and_then(|action| action.get("message"))
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| snippet(group, action))?;
    let mut event = Event::new("system.notify", &group.group_id);
    event.by = "system".into();
    event.data = json!({
        "kind": "automation_rule",
        "rule_id": rule_id,
        "text": message,
        "to": rule.get("to").cloned().unwrap_or_else(|| json!(["@all"])),
        "im_visibility": "public",
        "priority": action.and_then(|action| action.get("priority")).cloned().unwrap_or_else(|| json!("normal")),
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    Some(event)
}

fn snippet(group: &GroupDoc, action: Option<&Map<String, Value>>) -> Option<String> {
    let reference = action
        .and_then(|action| action.get("snippet_ref"))
        .and_then(Value::as_str)?;
    group
        .automation
        .get("snippets")
        .and_then(|value| value.get(reference))
        .or_else(|| {
            group
                .automation
                .get("snippet_overrides")
                .and_then(|value| value.get(reference))
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}

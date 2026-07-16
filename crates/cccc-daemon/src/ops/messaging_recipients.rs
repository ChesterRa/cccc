use cccc_core::GroupDoc;
use cccc_core::actors;
use serde_json::{Map, Value, json};

use crate::dispatch::OpError;

pub(super) fn normalize_chat_data(
    group: &GroupDoc,
    by: &str,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let text = data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let has_files = data
        .get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty());
    if text.is_empty() && !has_files {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }

    let raw = data
        .get("to")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut recipients = actors::resolve_recipients(group, &raw).map_err(OpError::invalid)?;
    if recipients.is_empty() && raw.is_empty() {
        recipients.push(default_recipient(group).into());
    }
    data.insert("to".into(), json!(recipients));
    data.entry("format")
        .or_insert_with(|| Value::String("plain".into()));
    data.entry("priority")
        .or_insert_with(|| Value::String("normal".into()));
    data.entry("reply_required").or_insert(Value::Bool(false));
    super::message_metadata::add_sender_snapshot(group, by, data);
    Ok(())
}

fn default_recipient(group: &GroupDoc) -> &'static str {
    let configured = group
        .extra
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| settings.get("default_send_to"))
        .or_else(|| {
            group
                .extra
                .get("messaging")
                .and_then(Value::as_object)
                .and_then(|settings| settings.get("default_send_to"))
        })
        .and_then(Value::as_str);
    if configured == Some("broadcast") {
        "@all"
    } else {
        "@foreman"
    }
}

use base64::Engine as _;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

use crate::dispatch::OpError;

fn require_recipients(payload: &Map<String, Value>) -> Result<(), OpError> {
    payload
        .get("to")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str().is_some_and(|value| !value.trim().is_empty()))
        })
        .then_some(())
        .ok_or_else(|| {
            OpError::new(
                "missing_remote_recipient",
                "remote_send requires explicit to across Group Bridge",
            )
        })
}

pub(super) fn normalize_outbound_payload(
    request: &DaemonRequest,
    payload: &mut Map<String, Value>,
) -> Result<(), OpError> {
    validate_remote_payload(payload)?;
    if payload
        .get("source_by")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        payload.insert(
            "source_by".into(),
            request
                .args
                .get("by")
                .cloned()
                .unwrap_or_else(|| json!("user")),
        );
    }
    let insight = cccc_core::peer_insight::normalize(request.args.get("insight"))
        .map_err(|message| OpError::new("invalid_insight", message))?;
    let required = request
        .args
        .get("require_peer_insight")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let peer_facing = payload
        .get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|recipient| !matches!(recipient.trim(), "" | "user" | "@user"));
    if required && peer_facing && insight.is_none() {
        let mut error = OpError::new(
            "peer_insight_required",
            "Not sent: this peer-facing message is missing `insight`.",
        );
        error
            .details
            .insert("delivery_state".into(), json!("not_sent"));
        error
            .details
            .insert("new_side_effects".into(), json!(false));
        error.details.insert(
            "recommended_action".into(),
            json!(cccc_core::peer_insight::PEER_INSIGHT_REQUIRED_ACTION.as_str()),
        );
        return Err(error);
    }
    Ok(())
}

pub(super) fn validate_remote_payload(payload: &mut Map<String, Value>) -> Result<(), OpError> {
    let legacy_fields = ["priority", "reply_required", "requires_ack"]
        .into_iter()
        .filter(|field| payload.contains_key(*field))
        .collect::<Vec<_>>();
    if !legacy_fields.is_empty() {
        return Err(OpError::new(
            "unsupported_message_fields",
            "use message_mode; legacy priority/reply_required/requires_ack fields are not supported",
        ));
    }
    require_recipients(payload)?;
    let Some(recipients) = payload.get("to").and_then(Value::as_array) else {
        return Err(OpError::new(
            "missing_remote_recipient",
            "remote_send requires explicit string recipients",
        ));
    };
    let recipients = recipients
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if recipients.is_empty() {
        return Err(OpError::new(
            "missing_remote_recipient",
            "remote_send requires explicit string recipients",
        ));
    }
    payload.insert("to".into(), json!(recipients));
    let message_mode = payload
        .get("message_mode")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(message_mode, "send" | "request_reply" | "mail") {
        return Err(OpError::new(
            "invalid_message_mode",
            "message_mode is required and must be send, request_reply, or mail",
        ));
    }
    crate::ops::messaging_recipients::validate_message_audience(&recipients, message_mode)?;
    if message_mode == "request_reply"
        && payload
            .get("to")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|recipient| matches!(recipient.trim(), "@all" | "@peers" | "@foreman"))
    {
        return Err(OpError::new(
            "concrete_recipients_required",
            "request_reply requires one or more explicit concrete recipients",
        ));
    }
    if payload
        .get("refs")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        return Err(OpError::new(
            "unsupported_refs",
            "refs are not supported by Group Bridge sessions",
        ));
    }
    if payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|value| !matches!(value, "plain" | "markdown"))
    {
        return Err(OpError::new(
            "invalid_payload",
            "format must be plain or markdown",
        ));
    }
    let has_text = payload
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_attachments = payload
        .get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    if !has_text && !has_attachments {
        return Err(OpError::new(
            "empty_message",
            "message text or attachments is required",
        ));
    }
    payload.entry("format").or_insert_with(|| json!("plain"));
    payload.entry("refs").or_insert_with(|| json!([]));
    payload.entry("attachments").or_insert_with(|| json!([]));
    Ok(())
}

pub(super) fn encode_outbound_attachments(
    home: &HomeLayout,
    group_id: &str,
    payload: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let Some(attachments) = payload.get_mut("attachments").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for attachment in attachments {
        let item = attachment
            .as_object_mut()
            .ok_or_else(|| OpError::new("invalid_attachments", "attachment must be an object"))?;
        if item
            .get("content_base64")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        {
            item.remove("path");
            continue;
        }
        let relative = item
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpError::new("invalid_attachments", "attachment path is required"))?;
        let path = cccc_core::blobs::resolve(home, group_id, relative)
            .map_err(|error| OpError::new("invalid_attachments", error.to_string()))?;
        let bytes = std::fs::read(path)
            .map_err(|error| OpError::new("invalid_attachments", error.to_string()))?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(OpError::new(
                "invalid_attachments",
                "remote attachment exceeds 10 MiB",
            ));
        }
        item.insert("bytes".into(), json!(bytes.len()));
        item.insert(
            "content_base64".into(),
            json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        );
        item.remove("path");
    }
    Ok(())
}

pub(super) fn store_remote_attachments(
    home: &HomeLayout,
    group_id: &str,
    payload: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let Some(attachments) = payload.get_mut("attachments").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for attachment in attachments {
        let item = attachment
            .as_object_mut()
            .ok_or_else(|| OpError::new("invalid_attachments", "attachment must be an object"))?;
        let Some(encoded) = item
            .remove("content_base64")
            .and_then(|value| value.as_str().map(str::to_owned))
        else {
            continue;
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| OpError::new("invalid_attachments", "invalid base64 attachment"))?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(OpError::new(
                "invalid_attachments",
                "remote attachment exceeds 10 MiB",
            ));
        }
        let blob = cccc_core::blobs::store(home, group_id, &bytes).map_err(OpError::io)?;
        item.insert("path".into(), json!(blob.path));
        item.insert("bytes".into(), json!(blob.bytes));
        item.insert("sha256".into(), json!(blob.sha256));
    }
    Ok(())
}

pub(super) fn remote_reply_recipients(source_by: &str) -> Vec<String> {
    let sender = source_by.trim();
    if matches!(sender, "user" | "@user") {
        return vec!["user".into()];
    }
    if sender.is_empty() || sender.starts_with(['@', '#']) || sender.starts_with("group_bridge:") {
        return Vec::new();
    }
    vec![sender.into()]
}

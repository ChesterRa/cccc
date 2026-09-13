use crate::dispatch::OpError;
use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};

pub(super) fn propagate(home: &HomeLayout, source: &Event, cancel: &Event) -> Value {
    match propagate_inner(home, source, cancel) {
        Ok(value) => value,
        Err(error) => json!({"state":"failed","error":{"code":error.code,"message":error.message}}),
    }
}
fn propagate_inner(home: &HomeLayout, source: &Event, cancel: &Event) -> Result<Value, OpError> {
    if source.by.starts_with("group_bridge:")
        || source.data.get("source_platform").and_then(Value::as_str)
            == Some("group_bridge_session")
    {
        return Ok(json!({"state":"not_applicable"}));
    }
    let Some(group_id) = source
        .data
        .get("dst_group_id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    else {
        return Ok(json!({"state":"not_applicable"}));
    };
    let Some(destination) =
        super::super::message_idempotency::find_relay(home, group_id, &source.id)
    else {
        return Ok(json!({"state":"not_applicable"}));
    };
    let event = append_destination_cancel(
        home,
        group_id,
        &destination.id,
        &source.group_id,
        &source.id,
        &cancel.id,
        "system",
    )?;
    Ok(json!({"state":"sent","transport":"local","event":event,"event_id":event.id}))
}

fn append_destination_cancel(
    home: &HomeLayout,
    target_group_id: &str,
    remote_source_event_id: &str,
    src_group_id: &str,
    source_message_event_id: &str,
    source_cancel_event_id: &str,
    by: &str,
) -> Result<Event, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(target_group_id).map_err(OpError::io)?;
    let source = ledger::find_event(&path, remote_source_event_id)
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "event_not_found",
                "relayed request-reply event was not found",
            )
        })?;
    if source.kind != "chat.message"
        || source.data.get("message_mode").and_then(Value::as_str) != Some("request_reply")
        || source.data.get("src_group_id").and_then(Value::as_str) != Some(src_group_id)
        || source.data.get("src_event_id").and_then(Value::as_str) != Some(source_message_event_id)
    {
        return Err(OpError::new(
            "source_event_mismatch",
            "relayed request-reply source does not match the cancellation provenance",
        ));
    }
    if let Some(existing) = ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .find(|event| {
            event.kind == "chat.reply_request.cancelled"
                && event.data.get("source_event_id").and_then(Value::as_str)
                    == Some(remote_source_event_id)
        })
    {
        return Ok(existing);
    }
    let mut event = Event::new("chat.reply_request.cancelled", target_group_id);
    event.by = by.into();
    event.scope_key = source.scope_key;
    event.data = json!({
        "source_event_id":remote_source_event_id,
        "src_group_id":src_group_id,
        "src_event_id":source_cancel_event_id,
        "src_message_event_id":source_message_event_id
    })
    .as_object()
    .cloned()
    .expect("reply cancellation data is an object");
    ledger::append(&path, &event).map_err(OpError::io)?;
    Ok(event)
}

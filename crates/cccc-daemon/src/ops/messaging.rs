use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, messaging_inbox};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "send" | "message_send" => send(home, request, "chat.message"),
        "send_cross_group" => send_cross_group(home, request),
        "send_cross_group_remote_record" => send_cross_group_remote_record(home, request),
        "tracked_send" => tracked_send(home, request),
        "slash_skill_dispatch" => slash_skill_dispatch(home, request),
        "reply" => reply(home, request),
        "stream_emit" => send(home, request, "chat.stream"),
        "system_notify" => send(home, request, "system.notify"),
        "event_append" => append_raw(home, request),
        "ledger_tail" => super::messaging_query::tail(home, request),
        "ledger_search" => super::messaging_query::search(home, request),
        "ledger_window" => super::messaging_query::window(home, request),
        "ledger_statuses" => super::messaging_status::statuses(home, request),
        "message_read_status" => super::messaging_status::read_status(home, request),
        "inbox_list" => messaging_inbox::list(home, request),
        "inbox_mark_read" => messaging_inbox::mark_read(home, request),
        "inbox_mark_all_read" => messaging_inbox::mark_all(home, request),
        "chat_ack" => messaging_inbox::ack(home, request, "chat.ack"),
        "notify_ack" => messaging_inbox::ack(home, request, "system.notify_ack"),
        _ => return None,
    })
}

fn send_cross_group_remote_record(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &source.group_id, "chat.message", &by, &request.args)
    {
        return object(
            json!({"source_event":event,"transport":"group_bridge_session","duplicate":true}),
        );
    }
    let text = string_arg(request, "text").unwrap_or_default();
    let attachments = request
        .args
        .get("attachments")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if text.trim().is_empty()
        && attachments
            .as_array()
            .is_none_or(|attachments| attachments.is_empty())
    {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }
    let mut data: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by" | "dst_group_id"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    super::messaging_recipients::normalize_remote_chat_data(&mut data)?;
    data.insert("dst_group_id".into(), json!(destination_id));
    data.insert("transport".into(), json!("group_bridge_session"));
    let event = append(home, &source.group_id, "chat.message", &by, data)?;
    object(json!({"source_event":event,"transport":"group_bridge_session"}))
}

fn send_cross_group(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    let destination = store(home)?
        .load(&destination_id)
        .map_err(OpError::not_found)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let text = string_arg(request, "text").unwrap_or_default();
    let attachments = request
        .args
        .get("attachments")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if text.trim().is_empty()
        && attachments
            .as_array()
            .is_none_or(|attachments| attachments.is_empty())
    {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }
    let existing_source = super::message_idempotency::find(
        home,
        &source.group_id,
        "chat.message",
        &by,
        &request.args,
    );
    if let Some(source_event) = existing_source.as_ref()
        && let Some(event) =
            super::message_idempotency::find_relay(home, &destination.group_id, &source_event.id)
    {
        return object(json!({
            "source_event":source_event,
            "event":event,
            "transport":"local",
            "duplicate":true
        }));
    }

    let destination_by = format!("{}::{}", source.group_id, by);
    let mut delivery_data: Map<String, Value> = existing_source.as_ref().map_or_else(
        || {
            request
                .args
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by" | "dst_group_id"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        },
        |event| event.data.clone(),
    );
    if existing_source.is_some() {
        // The accepted source event is authoritative on a relay retry.
        delivery_data.remove("require_peer_insight");
    }
    delivery_data.remove("transport");
    delivery_data.remove("to_group_id");
    super::messaging_recipients::normalize_chat_data(
        &destination,
        &destination_by,
        &mut delivery_data,
    )?;

    let source_event = if let Some(existing) = existing_source {
        existing
    } else {
        let mut source_data = delivery_data.clone();
        source_data.insert("to_group_id".into(), json!(destination.group_id));
        source_data.insert("transport".into(), json!("local"));
        append(home, &source.group_id, "chat.message", &by, source_data)?
    };

    let mut forwarded = request.clone();
    forwarded.args = delivery_data;
    forwarded
        .args
        .insert("group_id".into(), json!(destination.group_id));
    forwarded.args.insert("by".into(), json!(destination_by));
    forwarded
        .args
        .insert("src_group_id".into(), json!(source.group_id));
    forwarded
        .args
        .insert("src_event_id".into(), json!(source_event.id));
    let destination_response = send(home, &forwarded, "chat.message")?;
    object(json!({
        "source_event":source_event,
        "event":destination_response.get("event"),
        "transport":"local"
    }))
}

fn send(home: &HomeLayout, request: &DaemonRequest, kind: &str) -> OpResult {
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, kind, &by, &request.args)
    {
        return object(json!({
            "event":event,
            "delivery":{"accepted":true,"state":"duplicate","targeted":0,"online":0,"queued":0},
            "duplicate":true
        }));
    }
    let mut data: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if kind == "chat.message" {
        super::messaging_recipients::normalize_chat_data(&group, &by, &mut data)?;
    }
    let event = append(home, &group.group_id, kind, &by, data)?;
    let delivery = actor_delivery::dispatch(home, &group, &event);
    object(json!({"event": event, "delivery": delivery}))
}

fn tracked_send(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let mut forwarded = request.clone();
    let idempotency_key = string_arg(request, "idempotency_key").unwrap_or_default();
    if !idempotency_key.is_empty()
        && string_arg(request, "client_id")
            .unwrap_or_default()
            .is_empty()
    {
        let group_id = required_arg(request, "group_id")?;
        let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
        forwarded.args.insert(
            "client_id".into(),
            json!(super::message_idempotency::tracked_client_id(
                &group_id,
                &by,
                &idempotency_key,
            )),
        );
    }
    let response = send(home, &forwarded, "chat.message")?;
    object(json!({"event": response.get("event"), "delivery": response.get("delivery")}))
}

fn slash_skill_dispatch(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let capability_id = required_arg(request, "capability_id")?;
    cccc_core::capabilities::CapabilityStore::new(home.clone())
        .require(&capability_id)
        .map_err(OpError::not_found)?;
    let mut forwarded = request.clone();
    forwarded.args.insert(
        "text".into(),
        Value::String(required_arg(request, "task_text")?),
    );
    forwarded.args.insert(
        "control_kind".into(),
        Value::String("slash_skill_dispatch".into()),
    );
    forwarded
        .args
        .insert("title".into(), Value::String("slash_skill_dispatch".into()));
    forwarded
        .args
        .insert("capability_id".into(), Value::String(capability_id));
    send(home, &forwarded, "chat.message")
}

fn reply(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let reply_to = required_arg(request, "reply_to")?;
    let group = load(home, request)?;
    let target = find_event(home, &group.group_id, &reply_to)?;
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("reply_to".into(), Value::String(reply_to));
    super::message_metadata::add_reply_snapshot(&target, &mut forwarded.args);
    if !forwarded.args.contains_key("to") {
        forwarded.args.insert("to".into(), json!([target.by]));
    }
    send(home, &forwarded, "chat.message")
}

fn append_raw(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let raw = request
        .args
        .get("event")
        .ok_or_else(|| OpError::new("invalid_args", "event is required"))?;
    let event: Event = serde_json::from_value(raw.clone()).map_err(OpError::invalid)?;
    let path = store(home)?
        .ledger_path(&event.group_id)
        .map_err(OpError::io)?;
    if !path.exists() {
        return Err(OpError::new("group_not_found", "group not found"));
    }
    cccc_core::ledger::append(&path, &event).map_err(OpError::io)?;
    object(json!({"event": event}))
}

pub(super) fn append(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    by: &str,
    data: Map<String, Value>,
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = by.into();
    event.data = data;
    cccc_core::ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}

pub(super) fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}

pub(super) fn find_event(
    home: &HomeLayout,
    group_id: &str,
    event_id: &str,
) -> Result<Event, OpError> {
    cccc_core::ledger::find_event(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        event_id,
    )
    .map_err(OpError::io)?
    .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {event_id}")))
}

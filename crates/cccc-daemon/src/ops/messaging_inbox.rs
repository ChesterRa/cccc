use cccc_contracts::DaemonRequest;
use cccc_core::inbox;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, first_non_blank_arg, object, required_arg, string_arg};
use crate::ops::messaging::{append, find_event, load};

pub fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    authorize(&group, request, &actor_id)?;
    let limit = request
        .args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50) as usize;
    let mut messages = inbox::list_unread(home, &group, &actor_id, limit).map_err(OpError::io)?;
    match string_arg(request, "kind_filter").as_deref() {
        Some("chat") => messages.retain(|event| event.kind == "chat.message"),
        Some("notify") => messages.retain(|event| event.kind == "system.notify"),
        _ => {}
    }
    let cursor = inbox::cursor(home, &group.group_id, &actor_id).map_err(OpError::io)?;
    object(json!({"messages": messages, "cursor": {"event_id": cursor, "ts": ""}}))
}

pub fn mark_read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    let event_id = required_arg(request, "event_id")?;
    authorize(&group, request, &actor_id)?;
    inbox::mark_read(home, &group.group_id, &actor_id, &event_id).map_err(OpError::not_found)?;
    let event = append(
        home,
        &group.group_id,
        "chat.read",
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        json!({"actor_id": actor_id, "event_id": event_id})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )?;
    object(json!({"cursor": {"event_id": event_id, "ts": event.ts}, "event": event}))
}

pub fn mark_all(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    authorize(&group, request, &actor_id)?;
    let unread = inbox::list_unread(home, &group, &actor_id, 1000).map_err(OpError::io)?;
    let Some(last) = unread.last() else {
        return object(
            json!({"cursor": {"event_id": inbox::cursor(home, &group.group_id, &actor_id).map_err(OpError::io)?}, "event": null}),
        );
    };
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("event_id".into(), Value::String(last.id.clone()));
    mark_read(home, &forwarded)
}

pub fn ack(home: &HomeLayout, request: &DaemonRequest, kind: &str) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    let target_id = first_non_blank_arg(request, &["event_id", "notify_event_id"])
        .ok_or_else(|| OpError::new("invalid_args", "event_id is required"))?;
    let by = string_arg(request, "by").unwrap_or_else(|| actor_id.clone());
    if by != actor_id && by != "user" {
        return Err(OpError::new(
            "permission_denied",
            "ack must be performed by recipient",
        ));
    }
    find_event(home, &group.group_id, &target_id)?;
    let data = if kind == "chat.ack" {
        json!({"actor_id": actor_id, "event_id": target_id})
    } else {
        json!({"actor_id": actor_id, "notify_event_id": target_id})
    };
    let event = append(
        home,
        &group.group_id,
        kind,
        &by,
        data.as_object().cloned().unwrap_or_default(),
    )?;
    object(json!({"acked": true, "event": event}))
}

fn authorize(group: &GroupDoc, request: &DaemonRequest, actor_id: &str) -> Result<(), OpError> {
    permissions::require_inbox(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        actor_id,
    )
    .map_err(OpError::invalid)
}

use cccc_contracts::{DaemonRequest, Event};
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout, actors};
use cccc_core::{inbox, ledger};
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
    let kind_filter = kind_filter(request)?;
    let messages =
        inbox::list_unread(home, &group, &actor_id, limit, &kind_filter).map_err(OpError::io)?;
    let cursor = cursor_value(home, &group.group_id, &actor_id)?;
    object(json!({"messages": messages, "cursor": cursor}))
}

pub fn mark_read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    let event_id = required_arg(request, "event_id")?;
    authorize(&group, request, &actor_id)?;
    let target = validate_read_target(home, &group, &actor_id, &event_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    // Validate the shared cursor document before committing the read event.
    // Append first so a failed ledger write cannot make unread work disappear.
    inbox::cursor_details(home, &group.group_id, &actor_id).map_err(OpError::io)?;
    let event = append(
        home,
        &group.group_id,
        "chat.read",
        &by,
        json!({"actor_id": actor_id, "event_id": event_id})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )?;
    inbox::mark_read(home, &group.group_id, &actor_id, &event_id).map_err(OpError::io)?;
    let ack_event = if by == actor_id
        && target.kind == "chat.message"
        && target.data.get("priority").and_then(Value::as_str) == Some("attention")
    {
        ack(home, request, "chat.ack")?
            .get("event")
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let cursor = cursor_value(home, &group.group_id, &actor_id)?;
    object(json!({"cursor": cursor, "event": event, "ack_event": ack_event}))
}

pub fn mark_all(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    authorize(&group, request, &actor_id)?;
    let kind_filter = kind_filter(request)?;
    let Some(last) =
        inbox::latest_unread(home, &group, &actor_id, &kind_filter).map_err(OpError::io)?
    else {
        return object(
            json!({"cursor": cursor_value(home, &group.group_id, &actor_id)?, "event": null}),
        );
    };
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("event_id".into(), Value::String(last.id.clone()));
    mark_read(home, &forwarded)
}

fn cursor_value(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<Value, OpError> {
    let (event_id, ts, updated_at) =
        inbox::cursor_details(home, group_id, actor_id).map_err(OpError::io)?;
    Ok(json!({"event_id": event_id, "ts": ts, "updated_at": updated_at}))
}

fn validate_read_target(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    event_id: &str,
) -> Result<Event, OpError> {
    let target = find_event(home, &group.group_id, event_id)?;
    if !matches!(target.kind.as_str(), "chat.message" | "system.notify") {
        return Err(OpError::new(
            "invalid_event_kind",
            "event kind must be chat.message or system.notify",
        ));
    }
    if target.by == actor_id || !inbox::is_for_actor(group, &target, actor_id) {
        return Err(OpError::new(
            "event_not_for_actor",
            format!("event is not addressed to actor: {actor_id}"),
        ));
    }
    if actor_id == "user" {
        return Ok(target);
    }
    let path = crate::dispatch::store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let existed = ledger::inspect(&path, |events, positions| {
        let generations = inbox::actor_generation_positions(events);
        inbox::actor_generation_contains(&generations, positions, actor_id, &target)
    })
    .map_err(OpError::io)?
    .unwrap_or_else(|| {
        actors::find(group, actor_id)
            .is_some_and(|actor| actor.created_at.is_empty() || actor.created_at <= target.ts)
    });
    if !existed {
        return Err(OpError::new(
            "event_not_for_actor",
            format!("event predates the current actor generation: {actor_id}"),
        ));
    }
    Ok(target)
}

fn kind_filter(request: &DaemonRequest) -> Result<String, OpError> {
    let value = string_arg(request, "kind_filter").unwrap_or_else(|| "all".into());
    if matches!(value.as_str(), "all" | "chat" | "notify") {
        Ok(value)
    } else {
        Err(OpError::new(
            "invalid_kind_filter",
            "kind_filter must be all, chat, or notify",
        ))
    }
}

pub fn ack(home: &HomeLayout, request: &DaemonRequest, kind: &str) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    let target_id = first_non_blank_arg(request, &["event_id", "notify_event_id"])
        .ok_or_else(|| OpError::new("invalid_args", "event_id is required"))?;
    let by = string_arg(request, "by").unwrap_or_else(|| actor_id.clone());
    if by != actor_id {
        return Err(OpError::new(
            "permission_denied",
            "ack must be performed by recipient",
        ));
    }
    if actor_id != "user" && actors::find(&group, &actor_id).is_none() {
        return Err(OpError::new(
            "unknown_actor",
            format!("unknown actor: {actor_id}"),
        ));
    }
    authorize(&group, request, &actor_id)?;
    let target = find_event(home, &group.group_id, &target_id)?;
    if kind == "chat.ack" {
        if target.kind != "chat.message" {
            return Err(OpError::new(
                "invalid_event_kind",
                "event kind must be chat.message",
            ));
        }
        if target.by == actor_id {
            return Err(OpError::new(
                "cannot_ack_own_message",
                "cannot acknowledge your own message",
            ));
        }
        if target.data.get("priority").and_then(Value::as_str) != Some("attention") {
            return Err(OpError::new(
                "not_an_attention_message",
                "message priority is not attention",
            ));
        }
        let addressed = if actor_id == "user" {
            target
                .data
                .get("to")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|recipient| matches!(recipient, "user" | "@user"))
        } else {
            inbox::is_for_actor(&group, &target, &actor_id)
        };
        let path = crate::dispatch::store(home)?
            .ledger_path(&group.group_id)
            .map_err(OpError::io)?;
        let existed = ledger::inspect(&path, |events, positions| {
            let generations = inbox::actor_generation_positions(events);
            inbox::actor_generation_contains(&generations, positions, &actor_id, &target)
        })
        .map_err(OpError::io)?
        .unwrap_or_else(|| {
            actors::find(&group, &actor_id)
                .is_none_or(|actor| actor.created_at.is_empty() || actor.created_at <= target.ts)
        });
        if !addressed || !existed {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
        let already = ledger::inspect_status(&path, |_, _, acked_by, _| {
            acked_by
                .get(&target_id)
                .is_some_and(|actors| actors.contains(&actor_id))
        })
        .map_err(OpError::io)?;
        if already {
            return object(json!({"acked": true, "already": true, "event": null}));
        }
    } else {
        if target.kind != "system.notify" {
            return Err(OpError::new(
                "invalid_event_kind",
                "event kind must be system.notify",
            ));
        }
        if !inbox::is_for_actor(&group, &target, &actor_id) {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
    }
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
    if kind == "chat.ack" {
        object(json!({"acked": true, "already": false, "event": event}))
    } else {
        object(json!({"acked": true, "event": event}))
    }
}

fn authorize(group: &GroupDoc, request: &DaemonRequest, actor_id: &str) -> Result<(), OpError> {
    permissions::require_inbox(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        actor_id,
    )
    .map_err(OpError::invalid)
}

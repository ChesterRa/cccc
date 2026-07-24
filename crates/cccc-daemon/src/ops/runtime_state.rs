use cccc_contracts::{ActorRuntime, DaemonRequest, Event, RunnerKind, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::ops::messaging::append;

const KEY: &str = "runtime_states";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "headless_status" => headless_status(home, request),
        "headless_set_status" => headless_set_status(home, request),
        "headless_ack_message" => headless_ack_message(home, request),
        "runtime_wait_next_turn" | "web_model_runtime_wait_next_turn" => {
            wait_next_turn(home, request)
        }
        "runtime_complete_turn" | "web_model_runtime_complete_turn" => complete_turn(home, request),
        _ => return None,
    })
}

fn headless_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = actor(&group, &actor_id)?;
    if actor.runner != RunnerKind::Headless && actor.runtime != ActorRuntime::WebModel {
        return Err(OpError::new(
            "invalid_actor_runner",
            "headless operations require runner=headless or runtime=web_model",
        ));
    }
    if super::local_headless::supports(actor) {
        let state = super::local_headless::status(&group.group_id, &actor_id)
            .map(|state| serde_json::to_value(state).unwrap_or(Value::Null))
            .unwrap_or_else(|| default_state(&group, &actor_id));
        return object(json!({"state":state}));
    }
    let mut state = actor_state(home, &group.group_id, &actor_id)?;
    if state.is_null() {
        state = default_state(&group, &actor_id);
    }
    object(json!({"state":state}))
}

fn headless_set_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = actor(&group, &actor_id)?;
    if actor.runner != RunnerKind::Headless && actor.runtime != ActorRuntime::WebModel {
        return Err(OpError::new(
            "invalid_actor_runner",
            "headless operations require runner=headless or runtime=web_model",
        ));
    }
    if super::local_headless::supports(actor) {
        return Err(OpError::new(
            "provider_managed_headless",
            "local Codex/Claude headless status is managed by the daemon supervisor",
        ));
    }
    let status = required_arg(request, "status")?;
    if !matches!(status.as_str(), "idle" | "working" | "waiting" | "stopped") {
        return Err(OpError::new(
            "invalid_status",
            format!("invalid status: {status}"),
        ));
    }
    let task_id = request.args.get("task_id").cloned().unwrap_or(Value::Null);
    let state = update_actor_state(home, &group.group_id, &actor_id, |state| {
        ensure_state(state, &group, &actor_id);
        state["status"] = json!(status);
        state["task_id"] = task_id;
        state["updated_at"] = json!(utc_now());
        Ok(state.clone())
    })?;
    object(json!({"state":state}))
}

fn headless_ack_message(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    actor(&group, &actor_id)?;
    let message_id = required_arg(request, "message_id")?;
    let acked_at = utc_now();
    update_actor_state(home, &group.group_id, &actor_id, |state| {
        ensure_state(state, &group, &actor_id);
        state["last_message_id"] = json!(message_id);
        state["updated_at"] = json!(acked_at);
        Ok(())
    })?;
    object(json!({"message_id":message_id,"acked_at":acked_at}))
}

fn wait_next_turn(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = actor(&group, &actor_id)?;
    if !super::actor_runtime::is_structured(actor) {
        return Err(OpError::new(
            "invalid_actor_runner",
            "cccc_runtime_wait_next_turn requires runner=headless or runtime=web_model",
        ));
    }
    if super::local_headless::supports(actor) {
        return Err(OpError::new(
            "provider_managed_headless",
            "local Codex/Claude headless actors receive turns from the daemon supervisor",
        ));
    }
    let cursor = inbox::cursor(home, &group.group_id, &actor_id).map_err(OpError::io)?;
    if !actor.enabled
        || !group.running
        || matches!(
            group.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return object(
            json!({"status":"stopped","turn":null,"cursor":{"event_id":cursor,"ts":""},"instructions":"This CCCC structured actor is stopped."}),
        );
    }
    let limit = request
        .args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 20) as usize;
    let kind_filter = string_arg(request, "kind_filter").unwrap_or_else(|| "all".into());
    let mut messages = inbox::list_unread(home, &group, &actor_id, limit).map_err(OpError::io)?;
    match kind_filter.as_str() {
        "chat" => messages.retain(|event| event.kind == "chat.message"),
        "notify" => messages.retain(|event| event.kind == "system.notify"),
        _ => {}
    }
    if messages.is_empty() {
        set_runtime_status(home, &group, &actor_id, "waiting", "", "")?;
        return object(
            json!({"status":"idle","turn":null,"cursor":{"event_id":cursor,"ts":""},"suggested_retry_after_ms":5000}),
        );
    }
    let event_ids: Vec<_> = messages.iter().map(|event| event.id.clone()).collect();
    let latest = messages.last().expect("messages is not empty");
    let turn_id = turn_id(&group.group_id, &actor_id, &event_ids);
    let coalesced_text = coalesced_text(&messages, &actor_id);
    let turn = json!({
        "turn_id":turn_id,
        "group_id":group.group_id,
        "actor_id":actor_id,
        "created_at":utc_now(),
        "event_ids":event_ids,
        "latest_event_id":latest.id,
        "latest_ts":latest.ts,
        "messages":messages,
        "coalesced_text":coalesced_text,
        "system_prompt":cccc_core::system_prompt::render_session(home, &group, actor),
        "delivery":{"mode":"cursor_on_complete","cursor_committed":false,"max_events":limit,"kind_filter":kind_filter},
        "instructions":"Process this coalesced CCCC turn and call cccc_runtime_complete_turn when finished."
    });
    set_runtime_status(
        home,
        &group,
        &actor_id,
        "working",
        turn["turn_id"].as_str().unwrap_or(""),
        turn["latest_event_id"].as_str().unwrap_or(""),
    )?;
    object(json!({"status":"work_available","turn":turn,"cursor":{"event_id":cursor,"ts":""}}))
}

fn complete_turn(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = actor(&group, &actor_id)?;
    if !super::actor_runtime::is_structured(actor) {
        return Err(OpError::new(
            "invalid_actor_runner",
            "cccc_runtime_complete_turn requires runner=headless or runtime=web_model",
        ));
    }
    if super::local_headless::supports(actor) {
        return Err(OpError::new(
            "provider_managed_headless",
            "local Codex/Claude headless turns are completed by the daemon supervisor",
        ));
    }
    let by = string_arg(request, "by").unwrap_or_else(|| actor_id.clone());
    if by != actor_id {
        return Err(OpError::new(
            "permission_denied",
            "complete_turn must be called by the runtime actor",
        ));
    }
    if !actor.enabled
        || !group.running
        || matches!(
            group.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return Err(OpError::new(
            "actor_stopped",
            "structured actor is stopped; completion was not committed",
        ));
    }
    let status = string_arg(request, "status").unwrap_or_else(|| "done".into());
    if !matches!(status.as_str(), "done" | "partial" | "failed" | "cancelled") {
        return Err(OpError::new("invalid_status", "invalid completion status"));
    }
    let active_state = actor_state(home, &group.group_id, &actor_id)?;
    let active_turn_id = active_state["active_turn_id"].as_str().unwrap_or_default();
    let completed_turn_id = string_arg(request, "turn_id").filter(|value| !value.is_empty());
    if active_turn_id.is_empty()
        || completed_turn_id
            .as_deref()
            .is_some_and(|turn_id| turn_id != active_turn_id)
    {
        return Err(OpError::new(
            "stale_turn",
            "turn_id does not match the actor's active structured turn",
        ));
    }
    let mut event_ids: Vec<String> = request
        .args
        .get("event_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if event_ids.is_empty()
        && let Some(latest) = string_arg(request, "latest_event_id")
        && !latest.is_empty()
    {
        event_ids.push(latest);
    }
    if event_ids.is_empty() {
        return Err(OpError::new("missing_event_ids", "event_ids is required"));
    }
    let unread = inbox::list_unread(home, &group, &actor_id, 1000).map_err(OpError::io)?;
    validate_completed_prefix(&unread, &event_ids)?;
    let mut cursor_committed = false;
    let mut read_event = Value::Null;
    if matches!(status.as_str(), "done" | "partial") {
        let latest = event_ids.last().expect("event_ids is not empty");
        inbox::mark_read(home, &group.group_id, &actor_id, latest).map_err(OpError::not_found)?;
        read_event = serde_json::to_value(append(
            home,
            &group.group_id,
            "chat.read",
            &actor_id,
            json!({"actor_id":actor_id,"event_id":latest})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )?)
        .map_err(OpError::invalid)?;
        cursor_committed = true;
    }
    set_runtime_status(home, &group, &actor_id, "waiting", "", "")?;
    let cursor = inbox::cursor(home, &group.group_id, &actor_id).map_err(OpError::io)?;
    object(json!({
        "status":status,
        "turn_id":completed_turn_id.unwrap_or_else(|| active_turn_id.to_owned()),
        "cursor_committed":cursor_committed,
        "cursor":{"event_id":cursor,"ts":""},
        "read_event":read_event,
        "ack_events":[],
        "processed_event_ids":event_ids,
        "followup_delivery_scheduled":false,
        "summary":string_arg(request,"summary").unwrap_or_default()
    }))
}

fn group_actor(home: &HomeLayout, request: &DaemonRequest) -> Result<(GroupDoc, String), OpError> {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = string_arg(request, "actor_id")
        .or_else(|| string_arg(request, "by"))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "actor_id is required"))?;
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    Ok((group, actor_id))
}

fn actor<'a>(group: &'a GroupDoc, actor_id: &str) -> Result<&'a cccc_contracts::Actor, OpError> {
    group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))
}

fn actor_state(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<Value, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    Ok(integration_state::group_get(&store, group_id, KEY)
        .map_err(OpError::io)?
        .get(actor_id)
        .cloned()
        .unwrap_or(Value::Null))
}

fn update_actor_state<T>(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, group_id, KEY, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let states = value.as_object_mut().expect("runtime state initialized");
        change(states.entry(actor_id).or_insert(Value::Null))
    })
    .map_err(OpError::io)
}

fn ensure_state(state: &mut Value, group: &GroupDoc, actor_id: &str) {
    if state.is_null() {
        *state = default_state(group, actor_id);
    }
}

fn default_state(group: &GroupDoc, actor_id: &str) -> Value {
    let enabled = actor(group, actor_id).is_ok_and(|actor| actor.enabled);
    json!({
        "group_id":group.group_id,
        "actor_id":actor_id,
        "status":if enabled {"idle"} else {"stopped"},
        "task_id":null,
        "last_message_id":"",
        "active_turn_id":"",
        "latest_event_id":"",
        "updated_at":utc_now()
    })
}

fn set_runtime_status(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    status: &str,
    active_turn_id: &str,
    latest_event_id: &str,
) -> Result<(), OpError> {
    update_actor_state(home, &group.group_id, actor_id, |state| {
        ensure_state(state, group, actor_id);
        state["status"] = json!(status);
        state["active_turn_id"] = json!(active_turn_id);
        state["latest_event_id"] = json!(latest_event_id);
        state["updated_at"] = json!(utc_now());
        Ok(())
    })
}

fn validate_completed_prefix(unread: &[Event], event_ids: &[String]) -> Result<(), OpError> {
    for event_id in event_ids {
        if !unread.iter().any(|event| &event.id == event_id) {
            return Err(OpError::new(
                "turn_not_unread",
                format!("event is not currently unread: {event_id}"),
            ));
        }
    }
    let latest = event_ids.last().expect("event_ids is not empty");
    let prefix: Vec<_> = unread
        .iter()
        .take_while(|event| event.id != *latest)
        .map(|event| event.id.as_str())
        .chain(std::iter::once(latest.as_str()))
        .collect();
    let missing: Vec<_> = prefix
        .into_iter()
        .filter(|id| !event_ids.iter().any(|event_id| event_id == id))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(OpError::new(
            "non_contiguous_turn_events",
            format!("missing unread event ids: {}", missing.join(", ")),
        ))
    }
}

fn turn_id(group_id: &str, actor_id: &str, event_ids: &[String]) -> String {
    let digest = Sha256::digest(
        serde_json::to_vec(&json!({"group_id":group_id,"actor_id":actor_id,"event_ids":event_ids}))
            .unwrap_or_default(),
    );
    format!("webturn:{actor_id}:{digest:x}")[..actor_id.len() + 29].to_owned()
}

fn coalesced_text(messages: &[Event], actor_id: &str) -> String {
    let mut output = messages
        .iter()
        .map(|event| {
            let text = event.data.get("text").and_then(Value::as_str).unwrap_or("");
            format!("[{} -> {}] {}", event.by, actor_id, text)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if output.chars().count() > 24_000 {
        output = output.chars().take(23_920).collect();
        output.push_str("\n\n[cccc] coalesced turn text truncated");
    }
    output
}

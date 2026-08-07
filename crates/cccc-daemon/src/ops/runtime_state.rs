use cccc_contracts::{ActorRuntime, DaemonRequest, Event, RunnerKind, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io;

use crate::dispatch::{OpError, OpResult, first_non_blank_arg, object, required_arg, string_arg};

const KEY: &str = "runtime_states";
const DELIVERY_PREFERENCES_KEY: &str = "web_model_delivery_preferences";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "headless_status" => headless_status(home, request),
        "headless_set_status" => headless_set_status(home, request),
        "headless_ack_message" => headless_ack_message(home, request),
        "web_model_delivery_preferences_get" => delivery_preferences_get(home, request),
        "web_model_delivery_preferences_update" => delivery_preferences_update(home, request),
        "runtime_wait_next_turn" | "web_model_runtime_wait_next_turn" => {
            wait_next_turn(home, request)
        }
        "web_model_runtime_recover_turn" => recover_turn(home, request),
        "runtime_complete_turn" | "web_model_runtime_complete_turn" => complete_turn(home, request),
        _ => return None,
    })
}

fn delivery_preference(group: &GroupDoc, actor_id: &str) -> Value {
    let stored = group
        .extra
        .get(DELIVERY_PREFERENCES_KEY)
        .and_then(|preferences| preferences.get(actor_id));
    let mode = stored
        .and_then(|preference| preference.get("mode"))
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "standard" | "image_compat"))
        .unwrap_or("standard");
    json!({
        "mode":mode,
        "updated_at":stored.and_then(|value| value["updated_at"].as_str()).unwrap_or(""),
        "updated_by":stored.and_then(|value| value["updated_by"].as_str()).unwrap_or("")
    })
}

fn require_web_model_actor<'a>(
    group: &'a GroupDoc,
    actor_id: &str,
) -> Result<&'a cccc_contracts::Actor, OpError> {
    let actor = actor(group, actor_id)?;
    if actor.runtime != ActorRuntime::WebModel {
        return Err(OpError::new(
            "invalid_actor_runtime",
            "web-model delivery operations require runtime=web_model",
        ));
    }
    Ok(actor)
}

fn delivery_preferences_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    require_web_model_actor(&group, &actor_id)?;
    object(json!({
        "group_id":group.group_id,
        "actor_id":actor_id,
        "preference":delivery_preference(&group, &actor_id)
    }))
}

fn delivery_preferences_update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    require_web_model_actor(&group, &actor_id)?;
    let by = string_arg(request, "by").unwrap_or_default();
    if by != "user" {
        return Err(OpError::new(
            "permission_denied",
            "web-model delivery preferences are user-controlled",
        ));
    }
    let mode = required_arg(request, "mode")?.to_ascii_lowercase();
    if !matches!(mode.as_str(), "standard" | "image_compat") {
        return Err(OpError::new(
            "invalid_web_model_delivery_mode",
            "mode must be standard or image_compat",
        ));
    }
    let preference = json!({"mode":mode,"updated_at":utc_now(),"updated_by":by});
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, &group.group_id, DELIVERY_PREFERENCES_KEY, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        value
            .as_object_mut()
            .expect("preference map initialized")
            .insert(actor_id.clone(), preference.clone());
        Ok(())
    })
    .map_err(OpError::io)?;
    object(json!({
        "group_id":group.group_id,
        "actor_id":actor_id,
        "preference":preference
    }))
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
    let web_model_mode = delivery_preference(&group, &actor_id)["mode"].clone();
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
        "delivery":{"mode":"cursor_on_complete","cursor_committed":false,"max_events":limit,"kind_filter":kind_filter,"web_model_mode":web_model_mode},
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

fn recover_turn(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = require_web_model_actor(&group, &actor_id)?;
    let raw_event_ids = request
        .args
        .get("event_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            OpError::new(
                "invalid_event_ids",
                "event_ids must be a non-empty list of strings",
            )
        })?;
    if raw_event_ids.is_empty() || raw_event_ids.iter().any(|value| !value.is_string()) {
        return Err(OpError::new(
            "invalid_event_ids",
            "event_ids must be a non-empty list of strings",
        ));
    }
    let event_ids = raw_event_ids
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let requested = event_ids.iter().cloned().collect::<HashSet<_>>();
    if requested.len() != event_ids.len() || requested.contains("") {
        return Err(OpError::new(
            "invalid_event_ids",
            "event_ids must be non-empty and unique",
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let all_events = ledger::read_all(&ledger_path).map_err(OpError::io)?;
    let messages = all_events
        .iter()
        .filter(|event| requested.contains(&event.id))
        .cloned()
        .collect::<Vec<_>>();
    if messages.len() != requested.len() {
        let missing = event_ids
            .iter()
            .find(|event_id| !messages.iter().any(|event| &event.id == *event_id))
            .cloned()
            .unwrap_or_default();
        return Err(OpError::new(
            "event_not_found",
            format!("event not found: {missing}"),
        ));
    }
    for event in &messages {
        if !matches!(event.kind.as_str(), "chat.message" | "system.notify") {
            return Err(OpError::new(
                "invalid_event_kind",
                "turn event kind must be chat.message or system.notify",
            ));
        }
        if !inbox::is_for_actor(&group, event, &actor_id) {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
    }
    let latest = messages.last().expect("validated recovery messages");
    let cursor = inbox::cursor(home, &group.group_id, &actor_id).map_err(OpError::io)?;
    let cursor_position = cursor
        .as_deref()
        .and_then(|cursor_id| all_events.iter().position(|event| event.id == cursor_id));
    let latest_position = all_events.iter().position(|event| event.id == latest.id);
    if cursor_position
        .zip(latest_position)
        .is_none_or(|(cursor, latest)| cursor < latest)
    {
        return Err(OpError::new(
            "turn_not_committed",
            "turn recovery only accepts events already covered by the actor cursor",
        ));
    }
    let ordered_ids = messages
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let recovered_turn_id = turn_id(&group.group_id, &actor_id, &ordered_ids);
    let coalesced = coalesced_text(&messages, &actor_id);
    let web_model_mode = delivery_preference(&group, &actor_id)["mode"].clone();
    let turn = json!({
        "turn_id":recovered_turn_id,
        "group_id":group.group_id,
        "actor_id":actor_id,
        "created_at":utc_now(),
        "event_ids":ordered_ids,
        "latest_event_id":latest.id,
        "latest_ts":latest.ts,
        "messages":messages,
        "coalesced_text":coalesced,
        "system_prompt":cccc_core::system_prompt::render_session(home, &group, actor),
        "delivery":{"mode":"recovery_no_cursor_mutation","cursor_committed":true,"web_model_mode":web_model_mode}
    });
    object(json!({"status":"recovered","turn":turn}))
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
    let turn_id = string_arg(request, "turn_id")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| active_turn_id.to_owned());
    let raw_event_ids = request.args.get("event_ids").and_then(Value::as_array);
    if raw_event_ids.is_some_and(|items| items.iter().any(|item| !item.is_string())) {
        return Err(OpError::new(
            "invalid_event_ids",
            "event_ids must contain only strings",
        ));
    }
    let mut event_ids: Vec<String> = raw_event_ids
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
    let delivery_id = string_arg(request, "delivery_id")
        .filter(|value| !value.is_empty())
        .or_else(|| (request.op == "runtime_complete_turn").then(|| format!("runtime:{turn_id}")))
        .ok_or_else(|| OpError::new("missing_delivery_id", "delivery_id is required"))?;
    let completion = super::runtime_completion::Completion {
        turn_id: turn_id.clone(),
        event_ids: event_ids.clone(),
        status,
        delivery_id,
    };
    if let Some(receipt) =
        super::runtime_completion::find(home, &group.group_id, &actor_id, &completion)?
    {
        return finish_completion(home, &group, &actor_id, &completion, receipt, request);
    }
    if active_turn_id.is_empty() || turn_id != active_turn_id {
        return Err(OpError::new(
            "stale_turn",
            "turn_id does not match the actor's active structured turn",
        ));
    }
    let unread = inbox::list_unread(home, &group, &actor_id, 1000).map_err(OpError::io)?;
    validate_completed_prefix(&unread, &event_ids)?;
    let receipt = super::runtime_completion::append(home, &group.group_id, &actor_id, &completion)?;
    finish_completion(home, &group, &actor_id, &completion, receipt, request)
}

fn finish_completion(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    completion: &super::runtime_completion::Completion,
    receipt: Event,
    request: &DaemonRequest,
) -> OpResult {
    let cursor_committed = matches!(completion.status.as_str(), "done" | "partial");
    let runtime = actor_state(home, &group.group_id, actor_id)?;
    let owns_active_projection = runtime["active_turn_id"].as_str()
        == Some(completion.turn_id.as_str())
        && runtime["status"].as_str() == Some("working");
    if owns_active_projection {
        if cursor_committed {
            let latest = completion.event_ids.last().expect("event ids validated");
            inbox::mark_read(home, &group.group_id, actor_id, latest)
                .map_err(OpError::not_found)?;
        }
        set_runtime_status(home, group, actor_id, "waiting", "", "")?;
    }
    let cursor = inbox::cursor(home, &group.group_id, actor_id).map_err(OpError::io)?;
    object(json!({
        "status":completion.status,
        "turn_id":completion.turn_id,
        "delivery_id":completion.delivery_id,
        "cursor_committed":cursor_committed,
        "cursor":{"event_id":cursor,"ts":""},
        "read_event":receipt,
        "ack_events":[],
        "processed_event_ids":completion.event_ids,
        "followup_delivery_scheduled":false,
        "summary":string_arg(request,"summary").unwrap_or_default()
    }))
}

fn group_actor(home: &HomeLayout, request: &DaemonRequest) -> Result<(GroupDoc, String), OpError> {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = first_non_blank_arg(request, &["actor_id", "by"])
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
    let _ = actor_id;
    let mut output = super::actor_delivery_render::render_batch(messages).unwrap_or_default();
    if output.chars().count() > 24_000 {
        output = output.chars().take(23_920).collect();
        output.push_str("\n\n[cccc] coalesced turn text truncated");
    }
    output
}

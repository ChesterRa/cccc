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
const MAX_TURN_EVENTS: usize = 20;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "headless_status" => headless_status(home, request),
        "headless_set_status" => headless_set_status(home, request),
        "web_model_delivery_preferences_get" => delivery_preferences_get(home, request),
        "web_model_delivery_preferences_update" => delivery_preferences_update(home, request),
        "runtime_wait_next_turn" => wait_next_turn(home, request),
        "web_model_runtime_recover_turn" => recover_turn(home, request),
        "web_model_browser_delivery_record" => record_browser_delivery(home, request),
        "runtime_complete_turn" => complete_turn(home, request),
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

fn wait_next_turn(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| actor_id.clone());
    if by != actor_id {
        return Err(OpError::new(
            "permission_denied",
            "wait_next_turn must be called by the runtime actor",
        ));
    }
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
    if !actor.enabled
        || !group.running
        || matches!(
            group.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return object(
            json!({"status":"stopped","turn":null,"instructions":"This CCCC structured actor is stopped."}),
        );
    }
    let active_state = actor_state(home, &group.group_id, &actor_id)?;
    if active_state["status"] == "working"
        && active_state["active_turn_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    {
        return object(json!({
            "status":"turn_in_progress",
            "turn":null,
            "active_turn_id":active_state["active_turn_id"],
            "event_ids":active_state["active_event_ids"],
            "instructions":"Finish the active turn with cccc_runtime_complete_turn before requesting more work."
        }));
    }
    let limit = request
        .args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 20) as usize;
    if request.args.contains_key("kind_filter") {
        return Err(OpError::new(
            "unsupported_field",
            "runtime delivery does not support Inbox kind filters",
        ));
    }
    let transport = match string_arg(request, "transport") {
        None => "web_model_pull".to_owned(),
        Some(value) => match value.trim() {
            "" | "web_model_pull" => "web_model_pull".to_owned(),
            "web_model_browser" => "web_model_browser".to_owned(),
            _ => {
                return Err(OpError::new(
                    "invalid_transport",
                    "transport must be web_model_pull or web_model_browser",
                ));
            }
        },
    };
    let mut messages = Vec::new();
    for event in crate::ops::runtime_delivery::pending_sources(home, &group, actor, limit)? {
        if let Some((state, claimed_transport)) =
            crate::ops::runtime_delivery::latest_state(home, &group.group_id, &actor_id, &event.id)?
            && state == "claimed"
        {
            if claimed_transport == transport {
                messages.push(event);
            }
            continue;
        }
        match crate::ops::runtime_delivery::claim(
            home, &group, actor, &event.id, &transport, false,
        )? {
            crate::ops::runtime_delivery::ClaimResult::Claimed => messages.push(event),
            crate::ops::runtime_delivery::ClaimResult::Terminal(_) => {}
        }
    }
    if messages.is_empty() {
        set_runtime_status(home, &group, &actor_id, "waiting", "", "", &[])?;
        return object(json!({"status":"idle","turn":null,"suggested_retry_after_ms":5000}));
    }
    let event_ids: Vec<_> = messages.iter().map(|event| event.id.clone()).collect();
    let latest = messages.last().expect("messages is not empty");
    let turn_id = turn_id(&group.group_id, &actor_id, &event_ids);
    let coalesced_text = coalesced_text(home, &group, &messages, &actor_id);
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
        "delivery":{"mode":"runtime_delivery","transport":transport,"max_events":limit,"web_model_mode":web_model_mode},
        "instructions":"Process this coalesced CCCC turn and call cccc_runtime_complete_turn when finished."
    });
    set_runtime_status(
        home,
        &group,
        &actor_id,
        "working",
        turn["turn_id"].as_str().unwrap_or(""),
        turn["latest_event_id"].as_str().unwrap_or(""),
        &event_ids,
    )?;
    if transport == "web_model_pull" {
        for event_id in &event_ids {
            crate::ops::runtime_delivery::append_state(
                home,
                &group.group_id,
                &actor_id,
                &actor.created_at,
                event_id,
                &transport,
                crate::ops::runtime_delivery::DeliveryOutcome::Accepted,
            )?;
        }
    }
    object(json!({"status":"work_available","turn":turn}))
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
        if !turn_event_targets_actor(&group, event, &actor_id) {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
        let delivery = crate::ops::runtime_delivery::latest_state(
            home,
            &group.group_id,
            &actor_id,
            &event.id,
        )?;
        if !delivery.is_some_and(|(state, _)| matches!(state.as_str(), "accepted" | "ambiguous")) {
            return Err(OpError::new(
                "turn_not_delivered",
                format!("event was not handed to this runtime: {}", event.id),
            ));
        }
    }
    let latest = messages.last().expect("validated recovery messages");
    let ordered_ids = messages
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let recovered_turn_id = turn_id(&group.group_id, &actor_id, &ordered_ids);
    let coalesced = coalesced_text(home, &group, &messages, &actor_id);
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
        "delivery":{"mode":"recovery_no_delivery_mutation","web_model_mode":web_model_mode}
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
    if request.args.contains_key("latest_event_id") {
        return Err(OpError::new(
            "unsupported_field",
            "complete_turn requires the exact active event_ids",
        ));
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
    let event_ids: Vec<String> = raw_event_ids
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if event_ids.is_empty() {
        return Err(OpError::new("missing_event_ids", "event_ids is required"));
    }
    if event_ids.len() > MAX_TURN_EVENTS {
        return Err(OpError::new(
            "invalid_event_ids",
            format!("event_ids cannot contain more than {MAX_TURN_EVENTS} entries"),
        ));
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
    let active_event_ids = active_state["active_event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if event_ids != active_event_ids {
        return Err(OpError::new(
            "completion_conflict",
            "event_ids do not match the actor's active structured turn",
        ));
    }
    for event_id in &event_ids {
        let delivery =
            crate::ops::runtime_delivery::latest_state(home, &group.group_id, &actor_id, event_id)?;
        if !delivery.is_some_and(|(state, _)| matches!(state.as_str(), "accepted" | "ambiguous")) {
            return Err(OpError::new(
                "turn_not_delivered",
                format!("event was not handed to this runtime: {event_id}"),
            ));
        }
    }
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
    let runtime = actor_state(home, &group.group_id, actor_id)?;
    let owns_active_projection = runtime["active_turn_id"].as_str()
        == Some(completion.turn_id.as_str())
        && runtime["status"].as_str() == Some("working");
    if owns_active_projection {
        set_runtime_status(home, group, actor_id, "waiting", "", "", &[])?;
    }
    object(json!({
        "status":completion.status,
        "turn_id":completion.turn_id,
        "delivery_id":completion.delivery_id,
        "completion_event":receipt,
        "processed_event_ids":completion.event_ids,
        "followup_delivery_scheduled":false,
        "summary":string_arg(request,"summary").unwrap_or_default()
    }))
}

fn record_browser_delivery(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor_id) = group_actor(home, request)?;
    let actor = require_web_model_actor(&group, &actor_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| actor_id.clone());
    if by != actor_id {
        return Err(OpError::new(
            "permission_denied",
            "browser delivery records must be written by the runtime actor",
        ));
    }
    if request.args.contains_key("cursor_committed") {
        return Err(OpError::new(
            "unsupported_field",
            "browser delivery observations do not mutate the Mail cursor",
        ));
    }
    let turn_id = required_arg(request, "turn_id")?;
    let delivery_id = required_arg(request, "delivery_id")?;
    let event_ids = normalized_required_event_ids(request)?;
    let ledger_path = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let events = ledger::read_all(&ledger_path).map_err(OpError::io)?;
    for event_id in &event_ids {
        let Some(event) = events.iter().find(|event| event.id == *event_id) else {
            return Err(OpError::new(
                "event_not_found",
                format!("event not found: {event_id}"),
            ));
        };
        if !matches!(event.kind.as_str(), "chat.message" | "system.notify")
            || !turn_event_targets_actor(&group, event, &actor_id)
        {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
    }
    let browser_delivery = parse_browser_delivery(request)?
        .ok_or_else(|| OpError::new("missing_browser_delivery", "browser_delivery is required"))?;
    let event = super::runtime_completion::append_browser_delivery(
        home,
        &group.group_id,
        &actor_id,
        &turn_id,
        &event_ids,
        &delivery_id,
        &browser_delivery,
    )?;
    let reason = browser_delivery["detail"].as_str().unwrap_or_default();
    let runtime_outcome = match browser_delivery["state"].as_str().unwrap_or_default() {
        "submitted" | "bound" => Some((
            "accepted",
            crate::ops::runtime_delivery::DeliveryOutcome::Accepted,
        )),
        "ambiguous" => Some((
            "ambiguous",
            crate::ops::runtime_delivery::DeliveryOutcome::Ambiguous(reason),
        )),
        "failed" => Some((
            "failed",
            crate::ops::runtime_delivery::DeliveryOutcome::Failed(reason),
        )),
        "submitting" | "pending" | "" => None,
        _ => None,
    };
    if let Some((runtime_state, outcome)) = runtime_outcome {
        for source_event_id in &event_ids {
            let latest = crate::ops::runtime_delivery::latest_state(
                home,
                &group.group_id,
                &actor_id,
                source_event_id,
            )?;
            if latest
                .as_ref()
                .is_some_and(|(state, _)| state == runtime_state)
            {
                continue;
            }
            crate::ops::runtime_delivery::append_state(
                home,
                &group.group_id,
                &actor_id,
                &actor.created_at,
                source_event_id,
                "web_model_browser",
                outcome,
            )?;
        }
    }
    object(json!({"event":event}))
}

fn normalized_required_event_ids(request: &DaemonRequest) -> Result<Vec<String>, OpError> {
    let raw = request.args.get("event_ids").and_then(Value::as_array);
    if raw.is_some_and(|items| items.iter().any(|item| !item.is_string())) {
        return Err(OpError::new(
            "invalid_event_ids",
            "event_ids must contain only strings",
        ));
    }
    let event_ids = raw
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if event_ids.is_empty() {
        return Err(OpError::new("missing_event_ids", "event_ids is required"));
    }
    if event_ids.len() > MAX_TURN_EVENTS {
        return Err(OpError::new(
            "invalid_event_ids",
            format!("event_ids cannot contain more than {MAX_TURN_EVENTS} entries"),
        ));
    }
    Ok(event_ids)
}

fn parse_browser_delivery(request: &DaemonRequest) -> Result<Option<Value>, OpError> {
    let Some(raw) = request.args.get("browser_delivery") else {
        return Ok(None);
    };
    let Some(raw) = raw.as_object() else {
        return Err(OpError::new(
            "invalid_browser_delivery",
            "browser_delivery must be an object",
        ));
    };
    let state = raw
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if !matches!(
        state,
        "submitting" | "submitted" | "bound" | "pending" | "ambiguous" | "failed"
    ) {
        return Err(OpError::new(
            "invalid_browser_delivery_state",
            "browser delivery state must be submitting, submitted, bound, pending, ambiguous, or failed",
        ));
    }
    let detail = raw
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(4096)
        .collect::<String>();
    let mut normalized = json!({"state":state,"detail":detail});
    for field in [
        "provider",
        "target_url",
        "bound_conversation_url",
        "pending_conversation_url",
        "auto_bind_new_chat",
        "resolved_pending_new_chat",
    ] {
        if let Some(value) = raw.get(field) {
            normalized[field] = value.clone();
        }
    }
    Ok(Some(normalized))
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

pub(super) fn actor_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<Value, OpError> {
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
        "active_event_ids":[],
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
    event_ids: &[String],
) -> Result<(), OpError> {
    update_actor_state(home, &group.group_id, actor_id, |state| {
        ensure_state(state, group, actor_id);
        state["status"] = json!(status);
        state["active_turn_id"] = json!(active_turn_id);
        state["latest_event_id"] = json!(latest_event_id);
        state["active_event_ids"] = json!(event_ids);
        state["updated_at"] = json!(utc_now());
        Ok(())
    })
}

fn turn_event_targets_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    if event.kind == "system.notify"
        && matches!(
            event.data.get("kind").and_then(Value::as_str),
            Some("mail_notice" | "reply_notice")
        )
    {
        return event.data.get("target_actor_id").and_then(Value::as_str) == Some(actor_id);
    }
    inbox::is_for_actor(group, event, actor_id)
}

fn turn_id(group_id: &str, actor_id: &str, event_ids: &[String]) -> String {
    let digest = Sha256::digest(
        serde_json::to_vec(&json!({"group_id":group_id,"actor_id":actor_id,"event_ids":event_ids}))
            .unwrap_or_default(),
    );
    format!("webturn:{actor_id}:{digest:x}")[..actor_id.len() + 29].to_owned()
}

fn coalesced_text(
    home: &HomeLayout,
    group: &GroupDoc,
    messages: &[Event],
    actor_id: &str,
) -> String {
    let mut output = super::actor_delivery_render::render_batch_with_mail_context(
        home, group, actor_id, messages,
    )
    .unwrap_or_default();
    if output.chars().count() > 24_000 {
        const TRUNCATION: &str = "\n\n[cccc] coalesced turn text truncated";
        const HINT_MARKER: &str = "\n\n[cccc] MAIL PENDING:";
        let split_index = output.rfind(HINT_MARKER);
        let suffix = split_index
            .map(|index| output[index..].to_owned())
            .unwrap_or_default();
        let body = split_index.map_or(output.as_str(), |index| &output[..index]);
        let available = 24_000usize
            .saturating_sub(TRUNCATION.chars().count())
            .saturating_sub(suffix.chars().count())
            .max(1);
        let truncated = body.chars().take(available).collect::<String>();
        output = truncated;
        output.truncate(output.trim_end().len());
        output.push_str(TRUNCATION);
        output.push_str(&suffix);
    }
    output
}

//! Durable relay decisions built from the existing message, task, context, and delivery layers.
//! Original actor messages remain the visible artifacts. Structured ledger/context facts carry
//! only machine responsibility across model turns and process restarts.

use cccc_contracts::{ActorRole, ActorRuntime, DaemonRequest, Event, GroupState};
use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::fs::with_exclusive_lock;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox, ledger};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use super::operation::{Operation, Policy::Write};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

const HANDOFF_KIND: &str = "coordination.handoff";
const DECISION_KIND: &str = "coordination.decision";
const REMINDER_AFTER_SECONDS: i64 = 20;
const ESCALATION_AFTER_SECONDS: i64 = 30;
const MAX_SOURCE_EVENTS: usize = 200;

struct DecisionScope<'a> {
    home: &'a HomeLayout,
    group_id: &'a str,
    actor_id: &'a str,
    decision_id: &'a str,
    request_fingerprint: &'a str,
    source_event_ids: &'a [String],
    handoff_ids: &'a [String],
}

struct ContinueSpec<'a> {
    actor_id: &'a str,
    title: &'a str,
    text: &'a str,
}

fn relay_ledger_path(home: &HomeLayout, group_id: &str) -> Result<PathBuf, OpError> {
    GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .ledger_path(group_id)
        .map_err(OpError::io)
}

fn relay_lock_path(home: &HomeLayout, group_id: &str) -> Result<PathBuf, OpError> {
    let ledger_path = relay_ledger_path(home, group_id)?;
    Ok(ledger_path
        .parent()
        .expect("group ledger has a parent")
        .join("state/ledger/coordination_relay.lock"))
}

fn with_relay_lock<T>(
    home: &HomeLayout,
    group_id: &str,
    operation: impl FnOnce() -> Result<T, OpError>,
) -> Result<T, OpError> {
    let lock_path = relay_lock_path(home, group_id)?;
    with_exclusive_lock(&lock_path, || Ok(operation())).map_err(OpError::io)?
}

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    let handler = match request.op.as_str() {
        "coordination_decide" => decide,
        "coordination_relay_status" => status,
        "coordination_relay_remind" => remind_due,
        _ => return None,
    };
    Some(Operation::new(Write, handler))
}

/// Record the one durable review obligation created by a managed member turn.
/// The source report stays the human-readable truth; this event only adds machine ownership.
pub(super) fn record_handoff(
    home: &HomeLayout,
    group: &GroupDoc,
    source_actor_id: &str,
    target_actor_id: &str,
    turn_id: &str,
    source_events: &[Event],
    turn_status: &str,
) -> Result<Event, OpError> {
    with_relay_lock(home, &group.group_id, || {
        record_handoff_locked(
            home,
            group,
            source_actor_id,
            target_actor_id,
            turn_id,
            source_events,
            turn_status,
        )
    })
}

fn record_handoff_locked(
    home: &HomeLayout,
    group: &GroupDoc,
    source_actor_id: &str,
    target_actor_id: &str,
    turn_id: &str,
    source_events: &[Event],
    turn_status: &str,
) -> Result<Event, OpError> {
    if source_events.is_empty() {
        return Err(OpError::new(
            "relay_source_required",
            "relay handoff requires at least one source event",
        ));
    }
    let source_event_ids = source_events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let handoff_id = stable_id(
        "relay-handoff",
        &[&group.group_id, source_actor_id, target_actor_id, turn_id],
    );
    let event_id = stable_event_id(&handoff_id);
    let path = relay_ledger_path(home, &group.group_id)?;
    let summary = handoff_label(source_actor_id, turn_status, source_events.len());
    let task_ids = task_ids_from_events(source_events);
    let mut event =
        if let Some(existing) = ledger::find_event(&path, &event_id).map_err(OpError::io)? {
            if existing.kind != HANDOFF_KIND
                || existing.data.get("handoff_id").and_then(Value::as_str) != Some(&handoff_id)
            {
                return Err(OpError::new(
                    "relay_handoff_conflict",
                    "deterministic relay handoff id already belongs to different data",
                ));
            }
            existing
        } else {
            let mut event = Event::new(HANDOFF_KIND, &group.group_id);
            event.id = event_id;
            event.by = source_actor_id.into();
            event.data = json!({
                "handoff_id":handoff_id,
                "source_event_ids":source_event_ids,
                "source_actor_id":source_actor_id,
                "target_actor_id":target_actor_id,
                "turn_id":turn_id,
                "turn_status":turn_status,
                "summary":summary,
                "task_ids":task_ids,
                "status":"pending_review",
            })
            .as_object()
            .cloned()
            .expect("relay handoff data");
            ledger::append(&path, &event).map_err(OpError::io)?;
            event
        };
    let resolution = decision_for_handoff(&ledger::read_all(&path).map_err(OpError::io)?, &event);
    ensure_handoff_note(
        home,
        group,
        &event,
        if resolution.is_some() {
            "resolved"
        } else {
            "pending_review"
        },
        resolution.as_deref(),
        None,
    )?;
    // Keep the returned event canonical even when it came from an older retry.
    event
        .data
        .entry("summary")
        .or_insert_with(|| Value::String(summary));
    Ok(event)
}

fn decide(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    with_relay_lock(home, &group_id, || decide_locked(home, request))
}

fn decide_locked(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    let group = super::context::load_group(home, &group_id)?;
    let by = string_arg(request, "by")
        .or_else(|| string_arg(request, "actor_id"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    require_foreman(&group, &by)?;
    let decision = required_arg(request, "decision")?
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        decision.as_str(),
        "continue" | "wait_user" | "complete" | "blocked"
    ) {
        return Err(OpError::new(
            "invalid_relay_decision",
            "decision must be continue, wait_user, complete, or blocked",
        ));
    }
    let reason = string_arg(request, "reason")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if decision == "blocked" && reason.is_empty() {
        return Err(OpError::new(
            "relay_reason_required",
            "blocked requires a concrete reason",
        ));
    }
    if decision == "continue"
        && (!group.running || matches!(group.state, GroupState::Paused | GroupState::Stopped))
    {
        return Err(OpError::new(
            "relay_group_paused",
            "continue cannot dispatch new work while the user has paused or stopped the group",
        ));
    }
    let next_actor_id = if decision == "continue" {
        required_arg(request, "next_actor_id")?.trim().to_owned()
    } else {
        String::new()
    };
    let next_title = if decision == "continue" {
        required_arg(request, "next_title")?.trim().to_owned()
    } else {
        String::new()
    };
    let next_text = if decision == "continue" {
        required_arg(request, "next_text")?.trim().to_owned()
    } else {
        String::new()
    };
    if decision == "continue"
        && (next_actor_id.is_empty() || next_title.is_empty() || next_text.is_empty())
    {
        return Err(OpError::new(
            "relay_continue_incomplete",
            "continue requires non-empty next_actor_id, next_title, and next_text",
        ));
    }
    let request_fingerprint = decision_fingerprint(
        &decision,
        &reason,
        &next_actor_id,
        &next_title,
        &next_text,
        request,
    );
    let requested_event_ids = source_event_ids(request)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group_id).map_err(OpError::io)?;
    let mut events = ledger::read_all(&path).map_err(OpError::io)?;
    let source_event_ids = resolve_requested_sources(&group, &events, &requested_event_ids, &by)?;
    let sources = source_events(&group, &events, &source_event_ids, &by)?;
    ensure_implicit_handoffs(home, &group, &by, &sources)?;
    events = ledger::read_all(&path).map_err(OpError::io)?;
    let initial_handoffs = handoffs_for_sources(&events, &by, &source_event_ids);
    if initial_handoffs.is_empty() {
        return Err(OpError::new(
            "relay_handoff_missing",
            "source events do not belong to a relay handoff for this foreman",
        ));
    }
    // A report may be handled from the active Web turn just before its managed member turn
    // emits the final completion event. Replay that exact earlier decision, but never let it
    // silently absorb a later output from the same member turn.
    if let Some(existing) = overlapping_decision(&events, &source_event_ids) {
        let decided_sources = event_string_list(existing, "source_event_ids");
        if source_event_ids
            .iter()
            .any(|id| !decided_sources.contains(id))
        {
            return Err(OpError::new(
                "relay_decision_conflict",
                "this batch mixes a previous decision with other reports; decide the unhandled source events separately",
            ));
        }
        return replay_existing_decision(
            home,
            &group,
            existing,
            &by,
            &decision,
            &request_fingerprint,
        );
    }
    // A local model may produce several human-readable messages in one turn. Naming any one
    // unresolved output resolves every still-unhandled output in that machine handoff.
    let source_event_ids =
        complete_handoff_source_ids(&source_event_ids, &initial_handoffs, &events)?;
    let sources = source_events(&group, &events, &source_event_ids, &by)?;
    let handoffs = handoffs_for_sources(&events, &by, &source_event_ids);
    let handoff_ids = handoffs
        .iter()
        .filter_map(|event| event.data.get("handoff_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let decision_id = decision_id(&group_id, &by, &source_event_ids);
    let scope = DecisionScope {
        home,
        group_id: &group_id,
        actor_id: &by,
        decision_id: &decision_id,
        request_fingerprint: &request_fingerprint,
        source_event_ids: &source_event_ids,
        handoff_ids: &handoff_ids,
    };
    let event_id = stable_event_id(&decision_id);
    // The initial overlap check already rejected handled sources; expansion adds only unresolved IDs.

    let context_store = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let initial_document = context_store.load(&group_id).map_err(OpError::io)?;
    ensure_partial_decision_compatible(
        &events,
        &initial_document,
        &scope,
        &decision,
        &request_fingerprint,
    )?;
    let source_task_ids =
        source_task_ids(request, &sources, &handoffs, &initial_document, &events)?;
    let mut next_task_id = String::new();
    let mut visible_event_id = String::new();

    let responsibility = match decision.as_str() {
        "continue" => {
            if next_actor_id == by {
                return Err(OpError::new(
                    "relay_next_actor_invalid",
                    "continue must hand concrete work to another actor",
                ));
            }
            let next_actor = actors::find(&group, &next_actor_id).ok_or_else(|| {
                OpError::new(
                    "relay_next_actor_not_found",
                    format!("next actor not found: {next_actor_id}"),
                )
            })?;
            if !next_actor.enabled {
                return Err(OpError::new(
                    "relay_next_actor_stopped",
                    format!("next actor is disabled: {next_actor_id}"),
                ));
            }
            let tracked = tracked_continue(
                &scope,
                request,
                ContinueSpec {
                    actor_id: &next_actor_id,
                    title: &next_title,
                    text: &next_text,
                },
            )?;
            if tracked.get("message_sent").and_then(Value::as_bool) != Some(true)
                || tracked.get("partial_failure").and_then(Value::as_bool) == Some(true)
            {
                return Err(OpError::new(
                    "relay_continue_incomplete",
                    "the next task was not durably delivered; retry the same decision",
                ));
            }
            next_task_id = tracked
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            visible_event_id = tracked
                .get("event_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            json!({"kind":"actor","actor_id":next_actor_id,"task_id":next_task_id})
        }
        "wait_user" => json!({"kind":"user"}),
        "blocked" => json!({"kind":"external","reason":reason}),
        "complete" => json!({"kind":"none"}),
        _ => unreachable!(),
    };

    // tracked_send may have created the next task. Re-read the existing context and pin the
    // version used by both validation and commit so a concurrent user edit cannot be hidden.
    let document = context_store.load(&group_id).map_err(OpError::io)?;
    let context_version = context_store.version(&document).map_err(OpError::io)?;
    let decision_summary = decision_label(&decision, &reason, &next_actor_id, &next_title);
    let mut operations = task_decision_operations(&document, &source_task_ids, &decision, &reason);
    // Ledger and context share the same decision facts; the context only adapts note keys.
    let mut decision_data = json!({
        "decision_id":decision_id,
        "request_fingerprint":request_fingerprint,
        "by":by,
        "decision":decision,
        "summary":decision_summary,
        "reason":reason,
        "source_event_ids":source_event_ids,
        "handoff_ids":handoff_ids,
        "task_ids":source_task_ids,
        "next_task_id":next_task_id,
        "visible_event_id":visible_event_id,
        "responsibility":responsibility,
        "caller_may_idle":true,
        "safe_to_idle":false,
        "status":"applied",
    })
    .as_object()
    .cloned()
    .expect("relay decision data");
    operations.extend(resolved_handoff_notes(
        &handoffs,
        &decision_id,
        &by,
        &decision,
    ));
    operations.push(decision_note(&decision_data));

    let preview = context_store
        .sync(&group_id, &operations, Some(&context_version), &by, true)
        .map_err(context_sync_error)?;
    let other_handoffs = unresolved_handoffs(&events, &by)
        .into_iter()
        .filter(|event| {
            event
                .data
                .get("handoff_id")
                .and_then(Value::as_str)
                .is_some_and(|id| {
                    !handoff_ids.iter().any(|candidate| candidate == id)
                        && !escalation_for_handoff(&events, id)
                })
        })
        .count();
    let safe_to_idle = group_safe_to_idle(&preview.context, &decision, other_handoffs);
    let caller_may_idle = !group.running
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
        || actor_may_idle_in_context(&preview.context, &group, &by, other_handoffs);
    if decision == "complete" && !safe_to_idle {
        return Err(OpError::new(
            "relay_work_remains",
            "complete is not allowed while another live task or unresolved handoff remains",
        ));
    }
    // Replace the draft note with final observable responsibility fields before the one atomic
    // context write. Task operations remain idempotent, and note ids upsert on retries.
    decision_data.insert("safe_to_idle".into(), json!(safe_to_idle));
    decision_data.insert("caller_may_idle".into(), json!(caller_may_idle));
    if let Some(last) = operations.last_mut() {
        for field in ["safe_to_idle", "caller_may_idle", "visible_event_id"] {
            last.insert(field.into(), decision_data[field].clone());
        }
    }
    let result = context_store
        .sync(&group_id, &operations, Some(&context_version), &by, false)
        .map_err(context_sync_error)?;
    append_context_event(home, &group_id, &by, &result)?;

    let mut decision_event = Event::new(DECISION_KIND, &group_id);
    decision_event.id = event_id;
    decision_event.by = by.clone();
    decision_event.data = decision_data;
    // Publish the durable decision first. If acceptance is interrupted, bootstrap/status and the
    // existing supervisor reconcile it idempotently before the source can be dispatched again.
    ledger::append(&path, &decision_event).map_err(OpError::io)?;
    accept_handled_sources(
        home,
        &group,
        &by,
        &source_event_ids,
        &handoff_ids,
        &decision_id,
    )?;
    decision_result(home, &group, decision_event, false)
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    with_relay_lock(home, &group_id, || status_locked(home, request))
}

fn status_locked(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    let actor_id = required_arg(request, "actor_id")?.trim().to_owned();
    let group = super::context::load_group(home, &group_id)?;
    if actors::find(&group, &actor_id).is_none() {
        return Err(OpError::new(
            "relay_actor_not_found",
            format!("actor not found: {actor_id}"),
        ));
    }
    let path = relay_ledger_path(home, &group_id)?;
    let mut events = ledger::read_all(&path).map_err(OpError::io)?;
    if reconcile_recorded_decisions(home, &group, &events)? {
        events = ledger::read_all(&path).map_err(OpError::io)?;
    }
    reconcile_escalated_handoffs(home, &group, &events)?;
    let pending = unresolved_handoffs(&events, &actor_id)
        .into_iter()
        .map(|handoff| {
            let all_source_event_ids = event_string_list(&handoff, "source_event_ids");
            let source_event_ids = unresolved_source_ids(&events, &handoff);
            let delivery = source_event_ids
                .iter()
                .map(|source_event_id| {
                    json!({
                        "event_id":source_event_id,
                        "state":latest_delivery_state(&events, &actor_id, source_event_id),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "handoff_id":handoff.data.get("handoff_id").cloned().unwrap_or(Value::Null),
                "summary":handoff.data.get("summary").cloned().unwrap_or(Value::Null),
                "source_actor_id":handoff.data.get("source_actor_id").cloned().unwrap_or(Value::Null),
                "target_actor_id":actor_id,
                "turn_id":handoff.data.get("turn_id").cloned().unwrap_or(Value::Null),
                "task_ids":handoff.data.get("task_ids").cloned().unwrap_or_else(||json!([])),
                "source_event_ids":source_event_ids,
                "all_source_event_ids":all_source_event_ids,
                "delivery":delivery,
                "awaiting_user_intervention":escalation_for_handoff(&events, handoff.data.get("handoff_id").and_then(Value::as_str).unwrap_or_default()),
                "decision_required":!escalation_for_handoff(&events, handoff.data.get("handoff_id").and_then(Value::as_str).unwrap_or_default()),
                "decision_call":{
                    "tool":"cccc_coordination",
                    "action":"decide",
                    "event_ids":source_event_ids,
                    "decisions":["continue","wait_user","complete","blocked"],
                    "continue_requires":["next_actor_id","next_title","next_text"],
                    "blocked_requires":["reason"],
                }
            })
        })
        .collect::<Vec<_>>();
    let current = current_group_state(home, &group, &events)?;
    let responsibility_kind = current["responsibility"]["kind"]
        .as_str()
        .unwrap_or_default();
    let suspended = responsibility_kind == "user_pause";
    let awaiting_user_intervention = responsibility_kind == "user_intervention";
    let caller_may_idle = actor_may_idle_from_state(&current, &actor_id);
    object(json!({
        "pending":pending,
        "count":pending.len(),
        "requires_decision":!pending.is_empty() && !suspended && !awaiting_user_intervention,
        "awaiting_user_intervention":awaiting_user_intervention,
        "caller_may_idle":caller_may_idle,
        "safe_to_idle":current["safe_to_idle"],
        "responsibility":current["responsibility"],
        "responsibilities":current["responsibilities"],
        "instructions":if suspended {
            "The user paused or stopped this group. Preserve the handoff and do not restart work automatically."
        } else if pending.is_empty() {
            "No unresolved member handoff is waiting for this actor."
        } else if awaiting_user_intervention {
            "The Foreman received one decision reminder without recording responsibility. The handoff is preserved for user intervention; resume with an explicit relay decision when requested."
        } else {
            r#"Review the human-readable source output, then call cccc_coordination(action="decide", ...). Reading or replying alone does not resolve responsibility."#
        }
    }))
}

fn latest_delivery_state(events: &[Event], actor_id: &str, source_event_id: &str) -> String {
    events
        .iter()
        .rev()
        .find(|event| {
            event.kind == "runtime.delivery"
                && event.data.get("actor_id").and_then(Value::as_str) == Some(actor_id)
                && event.data.get("source_event_id").and_then(Value::as_str)
                    == Some(source_event_id)
        })
        .and_then(|event| event.data.get("state"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn remind_due(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    with_relay_lock(home, &group_id, || remind_due_locked(home, request))
}

fn remind_due_locked(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?.trim().to_owned();
    let actor_id = required_arg(request, "actor_id")?.trim().to_owned();
    let group = super::context::load_group(home, &group_id)?;
    let actor = actors::find(&group, &actor_id).ok_or_else(|| {
        OpError::new(
            "relay_actor_not_found",
            format!("actor not found: {actor_id}"),
        )
    })?;
    if actor.runtime != ActorRuntime::WebModel
        || !actor.enabled
        || !group.running
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
    {
        return object(json!({"reminded":false,"escalated":false,"reason":"actor_inactive"}));
    }
    let browser_idle = bool_arg(request, "browser_idle", false);
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group_id).map_err(OpError::io)?;
    let mut events = ledger::read_all(&path).map_err(OpError::io)?;
    if reconcile_recorded_decisions(home, &group, &events)? {
        events = ledger::read_all(&path).map_err(OpError::io)?;
    }
    reconcile_escalated_handoffs(home, &group, &events)?;
    let now = Utc::now();
    let mut reminders = Vec::new();
    let mut escalations = Vec::new();
    for handoff in unresolved_handoffs(&events, &actor_id) {
        let handoff_id = handoff
            .data
            .get("handoff_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if escalation_for_handoff(&events, handoff_id) {
            continue;
        }
        if let Some(reminder) = reminder_event_for_handoff(&events, handoff_id) {
            if !browser_idle {
                continue;
            }
            let Some(ready_at) =
                delivered_at(&events, &actor_id, std::slice::from_ref(&reminder.id))
            else {
                continue;
            };
            if now.signed_duration_since(ready_at).num_seconds() >= ESCALATION_AFTER_SECONDS {
                escalations.push(handoff);
            }
        } else {
            let source_ids = unresolved_source_ids(&events, &handoff);
            let Some(ready_at) = delivered_at(&events, &actor_id, &source_ids) else {
                continue;
            };
            if now.signed_duration_since(ready_at).num_seconds() >= REMINDER_AFTER_SECONDS {
                reminders.push(handoff);
            }
        }
        if reminders.len() >= 10 && escalations.len() >= 10 {
            break;
        }
    }

    let reminder_event = if reminders.is_empty() {
        None
    } else {
        Some(send_relay_notice(
            home,
            &group_id,
            &actor_id,
            &events,
            &reminders[..reminders.len().min(10)],
            RelayNotice::Reminder,
        )?)
    };
    let escalation_event = if escalations.is_empty() {
        None
    } else {
        let escalations = &escalations[..escalations.len().min(10)];
        let event = send_relay_notice(
            home,
            &group_id,
            &actor_id,
            &events,
            escalations,
            RelayNotice::Escalation,
        )?;
        for handoff in escalations {
            ensure_handoff_note(home, &group, handoff, "waiting_user", None, Some(&event.id))?;
        }
        Some(event)
    };
    if reminder_event.is_none() && escalation_event.is_none() {
        return object(json!({"reminded":false,"escalated":false,"reason":"none_due"}));
    }
    object(json!({
        "reminded":reminder_event.is_some(),
        "escalated":escalation_event.is_some(),
        "reminder_event":reminder_event,
        "escalation_event":escalation_event,
    }))
}

fn relay_batch_handoff_ids(handoffs: &[Event]) -> Vec<String> {
    handoffs
        .iter()
        .filter_map(|event| event.data.get("handoff_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn relay_batch_source_ids(events: &[Event], handoffs: &[Event]) -> Vec<String> {
    handoffs
        .iter()
        .flat_map(|event| unresolved_source_ids(events, event))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

enum RelayNotice {
    Reminder,
    Escalation,
}

fn send_relay_notice(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    events: &[Event],
    handoffs: &[Event],
    notice: RelayNotice,
) -> Result<Event, OpError> {
    let handoff_ids = relay_batch_handoff_ids(handoffs);
    let source_event_ids = relay_batch_source_ids(events, handoffs);
    let (label, recipient, text) = match notice {
        RelayNotice::Reminder => (
            "reminder",
            actor_id,
            format!(
                "[CCCC] A completed member handoff is still waiting for a recorded decision. Review the original report(s) already in this conversation, then call cccc_coordination(action=\"decide\", event_ids={source_event_ids:?}, decision=\"continue\"|\"wait_user\"|\"complete\"|\"blocked\"). Your normal reply remains the human-facing output; do not repeat the report in the tool call. Reading or replying alone does not resolve this handoff. continue must include next_actor_id, next_title, and next_text."
            ),
        ),
        RelayNotice::Escalation => (
            "escalation",
            "user",
            format!(
                "[CCCC] Collaboration is waiting for your decision. The web Foreman received {} completed member handoff(s) and one reminder, but did not record whether to continue, wait for you, complete, or mark the work blocked. The original reports are preserved under event IDs {source_event_ids:?}. Ask the Foreman to resume with an explicit relay decision. No model will be woken repeatedly while responsibility is with you.",
                handoffs.len()
            ),
        ),
    };
    let client_id = stable_id(
        &format!("relay-{label}"),
        &handoff_ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let mut args = json!({
        "group_id":group_id,"by":"system","to":[recipient],"message_mode":"send",
        "text":text,"client_id":client_id,"relay_kind":format!("decision_{label}"),
        "relay_handoff_ids":handoff_ids,"relay_source_event_ids":source_event_ids,
    })
    .as_object()
    .cloned()
    .expect("relay notice request");
    if matches!(notice, RelayNotice::Escalation) {
        args.insert("relay_actor_id".into(), json!(actor_id));
    }
    let request = DaemonRequest {
        v: 1,
        op: "send".into(),
        args,
    };
    let sent = super::messaging::send(home, &request, "chat.message")?;
    sent.get("event")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or_else(|| {
            OpError::new(
                format!("relay_{label}_failed"),
                format!("{label} returned no event"),
            )
        })
}

fn tracked_continue(
    scope: &DecisionScope<'_>,
    original: &DaemonRequest,
    next: ContinueSpec<'_>,
) -> Result<Map<String, Value>, OpError> {
    let mut refs = original
        .args
        .get("refs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    refs.push(json!({
        "kind":"relay_decision",
        "decision_id":scope.decision_id,
        "request_fingerprint":scope.request_fingerprint,
        "source_event_ids":scope.source_event_ids,
        "handoff_ids":scope.handoff_ids,
    }));
    let text = next.text.to_owned();
    let request = DaemonRequest {
        v: original.v,
        op: "tracked_send".into(),
        args: json!({
            "group_id":scope.group_id,
            "by":scope.actor_id,
            "to":[next.actor_id],
            "assignee":next.actor_id,
            "handoff_to":next.actor_id,
            "title":next.title,
            "text":text,
            "outcome":string_arg(original,"outcome").unwrap_or_else(|| next.text.into()),
            "status":"active",
            "waiting_on":"actor",
            "task_type":"standard",
            "task_priority":string_arg(original,"task_priority").unwrap_or_else(|| "normal".into()),
            "checklist":original.args.get("checklist").cloned().unwrap_or_else(|| json!([])),
            "refs":refs,
            "insight":string_arg(original,"insight").unwrap_or_default(),
            "idempotency_key":scope.decision_id,
        })
        .as_object()
        .cloned()
        .expect("relay tracked send"),
    };
    super::messaging::resolve_operation(&request)
        .expect("tracked_send is a messaging operation")
        .execute(scope.home, &request)
}

fn task_decision_operations(
    document: &ContextDoc,
    task_ids: &[String],
    decision: &str,
    reason: &str,
) -> Vec<Map<String, Value>> {
    let (status, waiting) = match decision {
        "continue" | "complete" => ("done", "none"),
        "wait_user" => ("active", "user"),
        "blocked" => ("active", "external"),
        _ => return Vec::new(),
    };
    let mut operations = Vec::new();
    for task_id in task_ids {
        let Some(task) = document
            .tasks
            .iter()
            .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id.as_str()))
        else {
            continue;
        };
        if task.get("status").and_then(Value::as_str) == Some("archived") {
            continue;
        }
        let mut update = Map::from_iter([
            ("op".into(), json!("task.update")),
            ("task_id".into(), json!(task_id)),
            ("handoff_to".into(), Value::Null),
            ("waiting_on".into(), json!(waiting)),
        ]);
        if decision == "blocked" {
            update.insert("notes".into(), json!(reason));
        }
        operations.push(update);
        operations.push(Map::from_iter([
            ("op".into(), json!("task.move")),
            ("task_id".into(), json!(task_id)),
            ("status".into(), json!(status)),
        ]));
    }
    operations
}

fn decision_note(data: &Map<String, Value>) -> Map<String, Value> {
    let mut note = data.clone();
    let id = note.remove("decision_id").expect("decision id");
    let tasks = note.remove("task_ids").expect("decision task ids");
    note.insert("op".into(), json!("coordination.relay.note"));
    note.insert("kind".into(), json!("decision"));
    note.insert("id".into(), id);
    note.insert(
        "task_id".into(),
        tasks
            .as_array()
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null),
    );
    note.insert(
        "next_actor_id".into(),
        data["responsibility"]
            .get("actor_id")
            .cloned()
            .unwrap_or(Value::Null),
    );
    for field in ["task_id", "next_task_id", "visible_event_id"] {
        if note[field] == "" {
            note.insert(field.into(), Value::Null);
        }
    }
    note
}

fn handoff_note_fields(handoff: &Event) -> Map<String, Value> {
    json!({
        "op":"coordination.relay.note", "kind":"handoff",
        "id":handoff.data.get("handoff_id").cloned().unwrap_or(Value::Null),
        "at":handoff.ts,
        "summary":handoff.data.get("summary").cloned().unwrap_or_else(|| json!("Relay handoff")),
        "task_id":handoff.data.get("task_ids").and_then(Value::as_array).and_then(|items| items.first()).cloned().unwrap_or(Value::Null),
        "source_event_ids":handoff.data.get("source_event_ids").cloned().unwrap_or_else(|| json!([])),
        "source_actor_id":handoff.data.get("source_actor_id").cloned().unwrap_or(Value::Null),
        "target_actor_id":handoff.data.get("target_actor_id").cloned().unwrap_or(Value::Null),
        "turn_id":handoff.data.get("turn_id").cloned().unwrap_or(Value::Null),
    }).as_object().cloned().expect("handoff note fields")
}

fn resolved_handoff_notes(
    handoffs: &[Event],
    decision_id: &str,
    by: &str,
    decision: &str,
) -> Vec<Map<String, Value>> {
    handoffs
        .iter()
        .map(|handoff| {
            let mut note = handoff_note_fields(handoff);
            note.extend([
                ("status".into(), json!("resolved")),
                ("decision".into(), json!(decision)),
                ("decision_id".into(), json!(decision_id)),
                ("resolved_at".into(), json!(cccc_contracts::utc_now())),
                ("resolved_by".into(), json!(by)),
            ]);
            note
        })
        .collect()
}

fn ensure_handoff_note(
    home: &HomeLayout,
    group: &GroupDoc,
    handoff: &Event,
    status: &str,
    decision_id: Option<&str>,
    escalation_event_id: Option<&str>,
) -> Result<(), OpError> {
    let handoff_id = handoff
        .data
        .get("handoff_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut operation = handoff_note_fields(handoff);
    operation.extend([
        ("id".into(), json!(handoff_id)),
        ("status".into(), json!(status)),
        (
            "decision_id".into(),
            decision_id.map(Value::from).unwrap_or(Value::Null),
        ),
        (
            "escalation_event_id".into(),
            escalation_event_id.map(Value::from).unwrap_or(Value::Null),
        ),
    ]);
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let current = contexts.load(&group.group_id).map_err(OpError::io)?;
    if let Some(existing) = current
        .coordination
        .get("recent_handoffs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|note| note.get("id").and_then(Value::as_str) == Some(handoff_id))
        && (existing.get("status").and_then(Value::as_str) == Some("resolved")
            || (existing.get("status").and_then(Value::as_str) == Some("waiting_user")
                && status == "pending_review")
            || (existing.get("status").and_then(Value::as_str) == Some(status)
                && existing.get("decision_id").and_then(Value::as_str)
                    == decision_id.filter(|value| !value.is_empty())
                && existing.get("escalation_event_id").and_then(Value::as_str)
                    == escalation_event_id.filter(|value| !value.is_empty())))
    {
        return Ok(());
    }
    let result = contexts
        .sync(&group.group_id, &[operation], None, &handoff.by, false)
        .map_err(OpError::invalid)?;
    append_context_event(home, &group.group_id, &handoff.by, &result)
}

fn context_sync_error(error: std::io::Error) -> OpError {
    if error.to_string() == "version_conflict" {
        OpError::new(
            "relay_context_changed",
            "the group context changed while this relay decision was being committed; retry the same decision",
        )
    } else {
        OpError::invalid(error)
    }
}

fn append_context_event(
    home: &HomeLayout,
    group_id: &str,
    by: &str,
    result: &cccc_core::context::ContextSyncResult,
) -> Result<(), OpError> {
    if result.changes.is_empty() {
        return Ok(());
    }
    let mut event = Event::new("context.sync", group_id);
    event.by = by.into();
    event.data = json!({"version":result.version,"changes":result.changes})
        .as_object()
        .cloned()
        .expect("context relay event");
    let path = relay_ledger_path(home, group_id)?;
    ledger::append(&path, &event).map_err(OpError::io)
}

fn reconcile_recorded_decisions(
    home: &HomeLayout,
    group: &GroupDoc,
    events: &[Event],
) -> Result<bool, OpError> {
    let mut changed = false;
    for decision in events.iter().filter(|event| event.kind == DECISION_KIND) {
        let actor_id = decision
            .data
            .get("by")
            .and_then(Value::as_str)
            .unwrap_or(&decision.by);
        if actors::find(group, actor_id).is_none() {
            continue;
        }
        let decision_id = decision
            .data
            .get("decision_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if decision_id.is_empty() {
            continue;
        }
        let source_event_ids = event_string_list(decision, "source_event_ids");
        let handoff_ids = event_string_list(decision, "handoff_ids");
        let handled_ids = handled_source_ids(events, &source_event_ids, &handoff_ids);
        if handled_ids
            .iter()
            .all(|event_id| latest_delivery_state(events, actor_id, event_id) == "accepted")
        {
            continue;
        }
        accept_handled_sources(
            home,
            group,
            actor_id,
            &source_event_ids,
            &handoff_ids,
            decision_id,
        )?;
        changed = true;
    }
    Ok(changed)
}

fn handled_source_ids(
    events: &[Event],
    source_event_ids: &[String],
    handoff_ids: &[String],
) -> Vec<String> {
    let mut ids = source_event_ids.iter().cloned().collect::<BTreeSet<_>>();
    for event in events {
        if event.kind == "chat.message"
            && event.data.get("relay_kind").and_then(Value::as_str) == Some("decision_reminder")
            && event
                .data
                .get("relay_handoff_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|id| handoff_ids.iter().any(|candidate| candidate == id))
        {
            ids.insert(event.id.clone());
        }
    }
    ids.into_iter().collect()
}

fn accept_handled_sources(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    source_event_ids: &[String],
    handoff_ids: &[String],
    decision_id: &str,
) -> Result<(), OpError> {
    let actor = actors::find(group, actor_id).ok_or_else(|| {
        OpError::new(
            "relay_actor_not_found",
            format!("actor not found: {actor_id}"),
        )
    })?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let events = ledger::read_all(&path).map_err(OpError::io)?;
    let handled_ids = handled_source_ids(&events, source_event_ids, handoff_ids);
    for source_event_id in &handled_ids {
        if latest_delivery_state(&events, actor_id, source_event_id) == "accepted" {
            continue;
        }
        super::runtime_delivery::append_state(
            home,
            &group.group_id,
            actor_id,
            &actor.created_at,
            source_event_id,
            "coordination_decision",
            super::runtime_delivery::DeliveryOutcome::Accepted,
        )?;
    }
    release_active_turn_if_handled(home, group, actor_id, &handled_ids, decision_id)?;
    Ok(())
}

fn release_active_turn_if_handled(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    handled_ids: &[String],
    decision_id: &str,
) -> Result<(), OpError> {
    let state = super::runtime_state::actor_state(home, &group.group_id, actor_id)?;
    if state["status"] != "working" {
        return Ok(());
    }
    let active_event_ids = state["active_event_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if active_event_ids.is_empty()
        || active_event_ids
            .iter()
            .any(|event_id| !handled_ids.iter().any(|handled| handled == event_id))
    {
        return Ok(());
    }
    let turn_id = state["active_turn_id"].as_str().unwrap_or_default();
    if turn_id.is_empty() {
        return Ok(());
    }
    let request = DaemonRequest {
        v: 1,
        op: "runtime_complete_turn".into(),
        args: json!({
            "group_id":group.group_id,
            "actor_id":actor_id,
            "by":actor_id,
            "turn_id":turn_id,
            "event_ids":active_event_ids,
            "delivery_id":format!("coordination:{decision_id}"),
            "status":"done",
            "summary":"Handled inside the active web-model turn by a durable relay decision."
        })
        .as_object()
        .cloned()
        .expect("relay completion request"),
    };
    super::runtime_state::resolve_operation(&request)
        .expect("runtime completion is a registered operation")
        .execute(home, &request)
        .map(|_| ())
}

fn decision_result(home: &HomeLayout, group: &GroupDoc, event: Event, replayed: bool) -> OpResult {
    let path = relay_ledger_path(home, &group.group_id)?;
    let events = ledger::read_all(&path).map_err(OpError::io)?;
    let current = current_group_state(home, group, &events)?;
    let actor_id = event
        .data
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or(&event.by);
    object(json!({
        "relay":event.data,
        "decision_event_id":event.id,
        "replayed":replayed,
        "caller_may_idle":actor_may_idle_from_state(&current, actor_id),
        "safe_to_idle":current["safe_to_idle"],
        "current_responsibility":current["responsibility"],
        "current_responsibilities":current["responsibilities"],
        "instructions":"The relay decision is durable. Original actor messages remain the human-facing output; this record only transfers or ends machine responsibility. Read is not acknowledgement. safe_to_idle is recomputed from current tasks, handoffs, and user pause state."
    }))
}

fn reconcile_escalated_handoffs(
    home: &HomeLayout,
    group: &GroupDoc,
    events: &[Event],
) -> Result<(), OpError> {
    for handoff in unresolved_handoffs_for_group(events) {
        let handoff_id = handoff
            .data
            .get("handoff_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(escalation) = escalation_event_for_handoff(events, handoff_id) else {
            let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
            let context = contexts.load(&group.group_id).map_err(OpError::io)?;
            let invalid_note = context
                .coordination
                .get("recent_handoffs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|note| {
                    note["id"] == handoff_id
                        && note["status"] == "waiting_user"
                        && note["escalation_event_id"]
                            .as_str()
                            .is_some_and(|id| !id.is_empty())
                })
                .and_then(Value::as_object);
            if let Some(note) = invalid_note {
                let mut repair = note.clone();
                repair.insert("op".into(), json!("coordination.relay.note"));
                repair.insert("kind".into(), json!("handoff"));
                repair.insert("status".into(), json!("pending_review"));
                repair.insert("escalation_event_id".into(), Value::Null);
                let result = contexts
                    .sync(&group.group_id, &[repair], None, &handoff.by, false)
                    .map_err(OpError::invalid)?;
                append_context_event(home, &group.group_id, &handoff.by, &result)?;
            }
            continue;
        };
        ensure_handoff_note(
            home,
            group,
            &handoff,
            "waiting_user",
            None,
            Some(&escalation.id),
        )?;
    }
    Ok(())
}

fn current_group_state(
    home: &HomeLayout,
    group: &GroupDoc,
    events: &[Event],
) -> Result<Value, OpError> {
    if !group.running || matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return Ok(json!({
            "safe_to_idle":true,
            "responsibility":{"kind":"user_pause"},
            "responsibilities":[{"kind":"user_pause"}]
        }));
    }

    let task_state = task_group_state(home, group)?;
    let task_responsibility = task_state["responsibility"].clone();
    let actor_work =
        (task_responsibility["kind"] == "actor_work").then_some(task_responsibility.clone());
    let unresolved = unresolved_handoffs_for_group(events);
    if unresolved.is_empty() {
        return Ok(task_state);
    }

    let (escalated, active): (Vec<_>, Vec<_>) = unresolved.iter().partition(|handoff| {
        escalation_for_handoff(
            events,
            handoff
                .data
                .get("handoff_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    });
    let intervention = (!escalated.is_empty()).then(|| {
        json!({
            "kind":"user_intervention",
            "handoff_ids":escalated.iter().filter_map(|event|event.data.get("handoff_id").and_then(Value::as_str)).collect::<Vec<_>>(),
            "reason":"foreman_decision_missing",
        })
    });

    if !active.is_empty() {
        let review = json!({
            "kind":"foreman_review",
            "handoff_ids":active.iter().filter_map(|event|event.data.get("handoff_id").and_then(Value::as_str)).collect::<Vec<_>>(),
            "actor_ids":active.iter().filter_map(|event|event.data.get("target_actor_id").and_then(Value::as_str)).collect::<BTreeSet<_>>(),
        });
        let mut responsibilities = vec![review.clone()];
        let mut state = json!({
            "safe_to_idle":false,
            "responsibility":review,
        });
        if let Some(actor_work) = actor_work {
            responsibilities.push(actor_work.clone());
            state["actor_work"] = actor_work;
        }
        if let Some(intervention) = intervention {
            responsibilities.push(intervention.clone());
            state["user_intervention"] = intervention;
        }
        if task_responsibility["kind"] != "none" && task_responsibility["kind"] != "actor_work" {
            responsibilities.push(task_responsibility.clone());
            state["waiting_responsibility"] = task_responsibility;
        }
        state["responsibilities"] = Value::Array(responsibilities);
        return Ok(state);
    }

    let intervention = intervention.expect("unresolved handoffs are all escalated");
    if let Some(actor_work) = actor_work {
        return Ok(json!({
            "safe_to_idle":false,
            "responsibility":actor_work,
            "user_intervention":intervention,
            "responsibilities":[actor_work, intervention],
        }));
    }
    let mut responsibilities = vec![intervention.clone()];
    if task_responsibility["kind"] != "none" {
        responsibilities.push(task_responsibility.clone());
    }
    Ok(json!({
        "safe_to_idle":task_state["safe_to_idle"],
        "responsibility":intervention,
        "waiting_responsibility":task_responsibility,
        "responsibilities":responsibilities,
    }))
}

fn task_group_state(home: &HomeLayout, group: &GroupDoc) -> Result<Value, OpError> {
    let document = ContextStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group.group_id)
        .map_err(OpError::io)?;
    let live = document
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("planned"),
                "done" | "archived"
            )
        })
        .collect::<Vec<_>>();
    if live.is_empty() {
        return Ok(json!({
            "safe_to_idle":true,
            "responsibility":{"kind":"none"},
            "responsibilities":[]
        }));
    }
    let foreman_id = actors::unique_available_foreman(group)
        .ok()
        .map(|actor| actor.id.as_str())
        .unwrap_or_default();
    let actor_work = live
        .iter()
        .filter(|task| {
            !matches!(
                task.get("waiting_on").and_then(Value::as_str),
                Some("user" | "external")
            ) && task
                .get("blocked_by")
                .and_then(Value::as_array)
                .is_none_or(|items| items.is_empty())
        })
        .map(|task| {
            let owner = ["assignee", "handoff_to"]
                .iter()
                .find_map(|field| task.get(*field).and_then(Value::as_str))
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(foreman_id);
            json!({
                "task_id":task.get("id").cloned().unwrap_or(Value::Null),
                "actor_id":if owner.is_empty(){Value::Null}else{json!(owner)},
                "waiting_on":task.get("waiting_on").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    if !actor_work.is_empty() {
        return Ok(json!({
            "safe_to_idle":false,
            "responsibility":{"kind":"actor_work","tasks":actor_work},
            "responsibilities":[{"kind":"actor_work","tasks":actor_work}]
        }));
    }
    let user_tasks = live
        .iter()
        .filter(|task| task.get("waiting_on").and_then(Value::as_str) == Some("user"))
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let external_tasks = live
        .iter()
        .filter(|task| {
            task.get("waiting_on").and_then(Value::as_str) == Some("external")
                || task
                    .get("blocked_by")
                    .and_then(Value::as_array)
                    .is_some_and(|items| !items.is_empty())
        })
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let responsibility = json!({
        "kind":if external_tasks.is_empty(){"user"}else if user_tasks.is_empty(){"external"}else{"user_and_external"},
        "user_task_ids":user_tasks,
        "external_task_ids":external_tasks,
    });
    Ok(json!({
        "safe_to_idle":true,
        "responsibility":responsibility,
        "responsibilities":[responsibility],
    }))
}

fn decided_ids(events: &[Event], field: &str) -> HashSet<String> {
    events
        .iter()
        .filter(|event| event.kind == DECISION_KIND)
        .flat_map(|event| event_string_list(event, field))
        .collect()
}

fn unresolved_handoffs_for_group(events: &[Event]) -> Vec<Event> {
    // Reuse this immutable ledger snapshot; never retain the sets across writes or requests.
    let handoffs = decided_ids(events, "handoff_ids");
    let sources = decided_ids(events, "source_event_ids");
    events
        .iter()
        .filter(|event| {
            event.kind == HANDOFF_KIND
                && !handoffs.contains(
                    event
                        .data
                        .get("handoff_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
                && event_string_list(event, "source_event_ids")
                    .iter()
                    .any(|id| !sources.contains(id))
        })
        .cloned()
        .collect()
}

fn actor_may_idle_in_context(
    document: &ContextDoc,
    group: &GroupDoc,
    actor_id: &str,
    other_handoffs: usize,
) -> bool {
    if other_handoffs > 0 {
        return false;
    }
    !document.tasks.iter().any(|task| {
        let live_actor_work = !matches!(
            task.get("status")
                .and_then(Value::as_str)
                .unwrap_or("planned"),
            "done" | "archived"
        ) && !matches!(
            task.get("waiting_on").and_then(Value::as_str),
            Some("user" | "external")
        ) && task
            .get("blocked_by")
            .and_then(Value::as_array)
            .is_none_or(|items| items.is_empty());
        if !live_actor_work {
            return false;
        }
        let owners = ["assignee", "handoff_to"]
            .iter()
            .filter_map(|field| task.get(*field).and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        owners.contains(&actor_id)
            || (owners.is_empty()
                && actors::effective_role(group, actor_id) == Some(ActorRole::Foreman))
    })
}

fn responsibility_blocks_actor(responsibility: &Value, actor_id: &str) -> bool {
    match responsibility["kind"].as_str().unwrap_or_default() {
        "foreman_review" => responsibility["actor_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|value| value.as_str() == Some(actor_id)),
        "actor_work" => responsibility["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|task| task.get("actor_id").and_then(Value::as_str) == Some(actor_id)),
        _ => false,
    }
}

fn actor_may_idle_from_state(current: &Value, actor_id: &str) -> bool {
    if let Some(responsibilities) = current["responsibilities"].as_array() {
        return !responsibilities
            .iter()
            .any(|responsibility| responsibility_blocks_actor(responsibility, actor_id));
    }
    !responsibility_blocks_actor(&current["responsibility"], actor_id)
        && !responsibility_blocks_actor(&current["actor_work"], actor_id)
}

fn group_safe_to_idle(document: &ContextDoc, decision: &str, other_handoffs: usize) -> bool {
    if decision == "continue" || other_handoffs > 0 {
        return false;
    }
    let mut live = document.tasks.iter().filter(|task| {
        !matches!(
            task.get("status")
                .and_then(Value::as_str)
                .unwrap_or("planned"),
            "done" | "archived"
        )
    });
    match decision {
        "complete" => live.count() == 0,
        "wait_user" | "blocked" => live.all(|task| {
            matches!(
                task.get("waiting_on").and_then(Value::as_str),
                Some("user" | "external")
            ) || task
                .get("blocked_by")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        }),
        _ => false,
    }
}

fn resolve_requested_sources(
    group: &GroupDoc,
    events: &[Event],
    requested: &[String],
    actor_id: &str,
) -> Result<Vec<String>, OpError> {
    let mut resolved = BTreeSet::new();
    for event_id in requested {
        let event = events
            .iter()
            .find(|event| &event.id == event_id)
            .ok_or_else(|| {
                OpError::new("event_not_found", format!("event not found: {event_id}"))
            })?;
        if event.kind == "chat.message"
            && event.data.get("relay_kind").and_then(Value::as_str) == Some("decision_reminder")
        {
            if !inbox::is_for_actor(group, event, actor_id) {
                return Err(OpError::new(
                    "relay_event_not_for_actor",
                    format!("reminder is not addressed to actor {actor_id}: {event_id}"),
                ));
            }
            resolved.extend(event_string_list(event, "relay_source_event_ids"));
        } else {
            resolved.insert(event_id.clone());
        }
    }
    let resolved = resolved.into_iter().collect::<Vec<_>>();
    validate_source_count(&resolved)?;
    Ok(resolved)
}

fn complete_handoff_source_ids(
    requested: &[String],
    handoffs: &[Event],
    events: &[Event],
) -> Result<Vec<String>, OpError> {
    let mut ids = requested.iter().cloned().collect::<BTreeSet<_>>();
    for handoff in handoffs {
        ids.extend(unresolved_source_ids(events, handoff));
    }
    let ids = ids.into_iter().collect::<Vec<_>>();
    validate_source_count(&ids)?;
    Ok(ids)
}

fn validate_source_count(event_ids: &[String]) -> Result<(), OpError> {
    if event_ids.is_empty() || event_ids.len() > MAX_SOURCE_EVENTS {
        return Err(OpError::new(
            "invalid_event_ids",
            format!("relay handoff must contain between 1 and {MAX_SOURCE_EVENTS} source events"),
        ));
    }
    Ok(())
}

fn source_events(
    group: &GroupDoc,
    events: &[Event],
    source_event_ids: &[String],
    actor_id: &str,
) -> Result<Vec<Event>, OpError> {
    let mut result = Vec::new();
    for event_id in source_event_ids {
        let event = events
            .iter()
            .find(|event| &event.id == event_id)
            .cloned()
            .ok_or_else(|| {
                OpError::new("event_not_found", format!("event not found: {event_id}"))
            })?;
        if !matches!(event.kind.as_str(), "chat.message" | "system.notify")
            || !inbox::is_for_actor(group, &event, actor_id)
        {
            return Err(OpError::new(
                "relay_event_not_for_actor",
                format!("event is not a relay source for actor {actor_id}: {event_id}"),
            ));
        }
        result.push(event);
    }
    Ok(result)
}

fn ensure_implicit_handoffs(
    home: &HomeLayout,
    group: &GroupDoc,
    target_actor_id: &str,
    sources: &[Event],
) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let events = ledger::read_all(&path).map_err(OpError::io)?;
    let covered = handoffs_for_sources(
        &events,
        target_actor_id,
        &sources
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .flat_map(|event| event_string_list(&event, "source_event_ids"))
    .collect::<HashSet<_>>();
    for source in sources {
        if covered.contains(&source.id) {
            continue;
        }
        if matches!(source.by.as_str(), "" | "user" | "system") || source.by == target_actor_id {
            return Err(OpError::new(
                "relay_handoff_missing",
                format!("event is not a member handoff: {}", source.id),
            ));
        }
        record_handoff_locked(
            home,
            group,
            &source.by,
            target_actor_id,
            &format!("implicit:{}", source.id),
            std::slice::from_ref(source),
            "reported",
        )?;
    }
    Ok(())
}

fn handoffs_for_sources(
    events: &[Event],
    target_actor_id: &str,
    source_ids: &[String],
) -> Vec<Event> {
    let wanted = source_ids.iter().collect::<HashSet<_>>();
    events
        .iter()
        .filter(|event| {
            event.kind == HANDOFF_KIND
                && event.data.get("target_actor_id").and_then(Value::as_str)
                    == Some(target_actor_id)
                && event_string_list(event, "source_event_ids")
                    .iter()
                    .any(|id| wanted.contains(id))
        })
        .cloned()
        .collect()
}

fn replay_existing_decision(
    home: &HomeLayout,
    group: &GroupDoc,
    existing: &Event,
    actor_id: &str,
    decision: &str,
    request_fingerprint: &str,
) -> OpResult {
    let stored_fingerprint = existing
        .data
        .get("request_fingerprint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let same = existing.data.get("decision").and_then(Value::as_str) == Some(decision)
        && existing.data.get("by").and_then(Value::as_str) == Some(actor_id)
        && (stored_fingerprint.is_empty() || stored_fingerprint == request_fingerprint);
    if !same {
        return Err(OpError::new(
            "relay_decision_conflict",
            "one or more source events already have a different relay decision",
        ));
    }
    let source_event_ids = event_string_list(existing, "source_event_ids");
    let handoff_ids = event_string_list(existing, "handoff_ids");
    let decision_id = existing
        .data
        .get("decision_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    accept_handled_sources(
        home,
        group,
        actor_id,
        &source_event_ids,
        &handoff_ids,
        decision_id,
    )?;
    decision_result(home, group, existing.clone(), true)
}

fn decision_for_handoff(events: &[Event], handoff: &Event) -> Option<String> {
    let handoff_id = handoff
        .data
        .get("handoff_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(decision) = events.iter().rev().find(|event| {
        event.kind == DECISION_KIND
            && event_string_list(event, "handoff_ids")
                .iter()
                .any(|id| id == handoff_id)
    }) {
        return decision
            .data
            .get("decision_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    if !unresolved_source_ids(events, handoff).is_empty() {
        return None;
    }
    let source_ids = event_string_list(handoff, "source_event_ids");
    events.iter().rev().find_map(|event| {
        (event.kind == DECISION_KIND
            && event_string_list(event, "source_event_ids")
                .iter()
                .any(|id| source_ids.iter().any(|source| source == id)))
        .then(|| {
            event
                .data
                .get("decision_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
    })
}

fn unresolved_source_ids(events: &[Event], handoff: &Event) -> Vec<String> {
    let handoff_id = handoff
        .data
        .get("handoff_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if decided_ids(events, "handoff_ids").contains(handoff_id) {
        return Vec::new();
    }
    let decided = decided_ids(events, "source_event_ids");
    event_string_list(handoff, "source_event_ids")
        .into_iter()
        .filter(|id| !decided.contains(id))
        .collect()
}

fn unresolved_handoffs(events: &[Event], target_actor_id: &str) -> Vec<Event> {
    unresolved_handoffs_for_group(events)
        .into_iter()
        .filter(|event| {
            event.data.get("target_actor_id").and_then(Value::as_str) == Some(target_actor_id)
        })
        .collect()
}

fn overlapping_decision<'a>(events: &'a [Event], source_ids: &[String]) -> Option<&'a Event> {
    let wanted = source_ids.iter().collect::<HashSet<_>>();
    events.iter().find(|event| {
        event.kind == DECISION_KIND
            && event_string_list(event, "source_event_ids")
                .iter()
                .any(|id| wanted.contains(id))
    })
}

fn delivered_at(
    events: &[Event],
    actor_id: &str,
    source_event_ids: &[String],
) -> Option<DateTime<Utc>> {
    let mut latest = None;
    for source_event_id in source_event_ids {
        let event = events.iter().rev().find(|event| {
            event.kind == "runtime.delivery"
                && event.data.get("actor_id").and_then(Value::as_str) == Some(actor_id)
                && event.data.get("source_event_id").and_then(Value::as_str)
                    == Some(source_event_id)
        })?;
        // Ambiguous protects against duplicate sends; it is not evidence of receipt.
        if event.data.get("state").and_then(Value::as_str) != Some("accepted") {
            return None;
        }
        let at = DateTime::parse_from_rfc3339(&event.ts)
            .ok()?
            .with_timezone(&Utc);
        latest = Some(latest.map_or(at, |current: DateTime<Utc>| current.max(at)));
    }
    latest
}

fn reminder_event_for_handoff<'a>(events: &'a [Event], handoff_id: &str) -> Option<&'a Event> {
    events.iter().rev().find(|event| {
        event.kind == "chat.message"
            && event.data.get("relay_kind").and_then(Value::as_str) == Some("decision_reminder")
            && event
                .data
                .get("relay_handoff_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|id| id == handoff_id)
    })
}

fn escalation_event_for_handoff<'a>(events: &'a [Event], handoff_id: &str) -> Option<&'a Event> {
    events
        .iter()
        .enumerate()
        .rev()
        .find_map(|(position, event)| {
            if event.kind != "chat.message"
                || event.data.get("relay_kind").and_then(Value::as_str)
                    != Some("decision_escalation")
                || !event_string_list(event, "relay_handoff_ids")
                    .iter()
                    .any(|id| id == handoff_id)
            {
                return None;
            }
            // Evaluate receipt evidence as it existed when this escalation was written.
            let preceding = &events[..position];
            let actor = event.data.get("relay_actor_id")?.as_str()?;
            delivered_at(
                preceding,
                actor,
                &event_string_list(event, "relay_source_event_ids"),
            )?;
            let reminder = reminder_event_for_handoff(preceding, handoff_id)?;
            delivered_at(preceding, actor, std::slice::from_ref(&reminder.id))?;
            Some(event)
        })
}

fn escalation_for_handoff(events: &[Event], handoff_id: &str) -> bool {
    escalation_event_for_handoff(events, handoff_id).is_some()
}

fn ensure_partial_decision_compatible(
    events: &[Event],
    document: &ContextDoc,
    scope: &DecisionScope<'_>,
    decision: &str,
    request_fingerprint: &str,
) -> Result<(), OpError> {
    if let Some((artifact_decision, artifact_fingerprint)) = events.iter().rev().find_map(|event| {
        if event.kind != "chat.message" {
            return None;
        }
        let relay_ref = event
            .data
            .get("refs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|item| {
                item.get("kind").and_then(Value::as_str) == Some("relay_decision")
                    && item.get("decision_id").and_then(Value::as_str) == Some(scope.decision_id)
            })?;
        let artifact_decision = event
            .data
            .get("relay_decision")
            .and_then(Value::as_str)
            .unwrap_or("continue")
            .to_owned();
        let fingerprint = event
            .data
            .get("relay_fingerprint")
            .or_else(|| relay_ref.get("request_fingerprint"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Some((artifact_decision, fingerprint))
    }) {
        if artifact_decision != decision
            || (!artifact_fingerprint.is_empty() && artifact_fingerprint != request_fingerprint)
        {
            return Err(OpError::new(
                "relay_decision_conflict",
                "this handoff already has a different partially committed relay decision",
            ));
        }
    }

    let tracked_client_id = super::message_idempotency::tracked_client_id(
        scope.group_id,
        scope.actor_id,
        scope.decision_id,
    );
    if document.tasks.iter().any(|task| {
        task.get("client_request_id").and_then(Value::as_str) == Some(tracked_client_id.as_str())
    }) && decision != "continue"
    {
        return Err(OpError::new(
            "relay_decision_conflict",
            "this handoff already created a next task and must be retried as continue",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decision_fingerprint(
    decision: &str,
    reason: &str,
    next_actor_id: &str,
    next_title: &str,
    next_text: &str,
    request: &DaemonRequest,
) -> String {
    let mut hasher = Sha256::new();
    for (name, value) in [
        ("decision", Value::String(decision.into())),
        ("reason", Value::String(reason.into())),
        ("next_actor_id", Value::String(next_actor_id.into())),
        ("next_title", Value::String(next_title.into())),
        ("next_text", Value::String(next_text.into())),
        (
            "task_id",
            string_arg(request, "task_id").map_or(Value::Null, Value::String),
        ),
        (
            "outcome",
            string_arg(request, "outcome").map_or(Value::Null, Value::String),
        ),
        (
            "task_priority",
            string_arg(request, "task_priority").map_or(Value::Null, Value::String),
        ),
        (
            "checklist",
            request
                .args
                .get("checklist")
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "insight",
            string_arg(request, "insight").map_or(Value::Null, Value::String),
        ),
    ] {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(&value).expect("relay intent is serializable"));
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    format!("relay-intent:{digest:.32x}")
}

fn source_task_ids(
    request: &DaemonRequest,
    sources: &[Event],
    handoffs: &[Event],
    document: &ContextDoc,
    events: &[Event],
) -> Result<Vec<String>, OpError> {
    let source_actors = handoffs
        .iter()
        .filter_map(|handoff| handoff.data.get("source_actor_id").and_then(Value::as_str))
        .chain(sources.iter().map(|source| source.by.as_str()))
        .filter(|actor_id| !actor_id.is_empty())
        .collect::<HashSet<_>>();
    let mut referenced = task_ids_from_events(sources)
        .into_iter()
        .collect::<BTreeSet<_>>();
    referenced.extend(
        handoffs
            .iter()
            .flat_map(|handoff| event_string_list(handoff, "task_ids")),
    );
    // Resolve the original assignment before considering any current worker task.
    for source in sources {
        if let Some(parent) = source
            .data
            .get("reply_to")
            .and_then(Value::as_str)
            .and_then(|id| events.iter().find(|event| event.id == id))
        {
            referenced.extend(task_ids_from_events(std::slice::from_ref(parent)));
        }
    }
    if let Some(task_id) = string_arg(request, "task_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        let task = document
            .tasks
            .iter()
            .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id.as_str()))
            .ok_or_else(|| {
                OpError::new("relay_task_not_found", format!("task not found: {task_id}"))
            })?;
        let source_owned = task
            .get("assignee")
            .and_then(Value::as_str)
            .is_some_and(|assignee| source_actors.contains(assignee));
        if !referenced.contains(&task_id) && !source_owned {
            return Err(OpError::new(
                "relay_task_not_owned",
                "task_id must be referenced by the handoff or assigned to its source member",
            ));
        }
        referenced.insert(task_id);
    }
    // Ownership requires an actual link: the report's task_ref, the original
    // assignment behind a reply, the handoff's recorded task_ids, or an explicit
    // task_id. A single active task owned by the source member is not evidence.
    let ids = referenced
        .into_iter()
        .filter(|id| {
            document
                .tasks
                .iter()
                .any(|task| task.get("id").and_then(Value::as_str) == Some(id.as_str()))
        })
        .collect::<Vec<_>>();
    Ok(ids)
}

fn task_ids_from_events(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .flat_map(|event| {
            event
                .data
                .get("refs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|item| item.get("kind").and_then(Value::as_str) == Some("task_ref"))
                .filter_map(|item| item.get("task_id").and_then(Value::as_str))
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn source_event_ids(request: &DaemonRequest) -> Result<Vec<String>, OpError> {
    let mut values = Vec::new();
    if let Some(event_id) = string_arg(request, "event_id") {
        values.push(event_id);
    }
    if let Some(items) = request.args.get("event_ids") {
        let array = items.as_array().ok_or_else(|| {
            OpError::new("invalid_event_ids", "event_ids must be an array of strings")
        })?;
        if array.iter().any(|item| !item.is_string()) {
            return Err(OpError::new(
                "invalid_event_ids",
                "event_ids must contain only strings",
            ));
        }
        values.extend(array.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    let mut seen = HashSet::new();
    let values = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect::<Vec<_>>();
    if values.is_empty() || values.len() > MAX_SOURCE_EVENTS {
        return Err(OpError::new(
            "invalid_event_ids",
            format!("provide between 1 and {MAX_SOURCE_EVENTS} unique source event ids"),
        ));
    }
    Ok(values)
}

fn require_foreman(group: &GroupDoc, actor_id: &str) -> Result<(), OpError> {
    if actors::effective_role(group, actor_id) == Some(ActorRole::Foreman) {
        return Ok(());
    }
    Err(OpError::new(
        "relay_decision_forbidden",
        "relay decisions require the current foreman",
    ))
}

fn handoff_label(source_actor_id: &str, status: &str, report_count: usize) -> String {
    format!(
        "{source_actor_id} ended this turn ({status}); {report_count} original report(s) preserved"
    )
}

fn decision_label(decision: &str, reason: &str, next_actor_id: &str, next_title: &str) -> String {
    match decision {
        "continue" => format!("Continue with {next_actor_id}: {next_title}"),
        "wait_user" => "Waiting for user".into(),
        "complete" => "Complete".into(),
        "blocked" => format!("Blocked: {reason}"),
        _ => decision.into(),
    }
}

fn event_string_list(event: &Event, field: &str) -> Vec<String> {
    event
        .data
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn decision_id(group_id: &str, actor_id: &str, event_ids: &[String]) -> String {
    let mut sorted = event_ids.to_vec();
    sorted.sort();
    let mut parts = vec![group_id, actor_id];
    parts.extend(sorted.iter().map(String::as_str));
    stable_id("relay-decision", &parts)
}

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let digest = Sha256::digest(parts.join("\0"));
    format!("{prefix}:{digest:.32x}")
}

fn stable_event_id(id: &str) -> String {
    format!("{:x}", Sha256::digest(id.as_bytes()))[..32].to_owned()
}

#[cfg(test)]
#[path = "coordination_relay_tests.rs"]
mod tests;

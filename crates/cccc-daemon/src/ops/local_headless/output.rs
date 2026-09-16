use super::{ActiveTurn, Session, events};
use serde_json::{Map, Value, json};

pub(super) fn handle_message(session: &Session, message: Value) {
    if message.get("id").is_some() {
        if message.get("method").and_then(Value::as_str).is_some() {
            respond_unsupported_server_request(session, &message);
        }
        return;
    }
    handle_announced_message(session, message);
}

fn respond_unsupported_server_request(session: &Session, message: &Value) {
    let Some(id) = message.get("id") else {
        return;
    };
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let _ = session.respond_error(
        id.clone(),
        json!({
            "code":-32601,
            "message":format!("CCCC headless does not support provider request: {method}")
        }),
    );
}

fn handle_announced_message(session: &Session, message: Value) {
    if message.get("method").and_then(Value::as_str) == Some("turn/started") {
        handle_managed_turn_started(session, &message);
        return;
    }
    let completed = message.get("method").and_then(Value::as_str) == Some("turn/completed");
    if completed {
        complete_turn(session, &message);
        return;
    }
    if message.get("method").and_then(Value::as_str) == Some("thread/status/changed") {
        let flags = message
            .pointer("/params/status/activeFlags")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let waiting = flags.iter().any(|flag| {
            matches!(
                flag.as_str(),
                Some("waitingOnApproval" | "waitingOnUserInput")
            )
        });
        let task = active_context(session);
        if waiting {
            session.set_status("waiting", task);
        } else if message
            .pointer("/params/status/type")
            .and_then(Value::as_str)
            == Some("active")
            && task.is_some()
            && session
                .status
                .lock()
                .is_ok_and(|state| state.status == "waiting")
        {
            session.set_status("working", task);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartedTurnDisposition {
    Adopted,
    Matched,
    Conflict,
}

fn handle_managed_turn_started(session: &Session, message: &Value) {
    let turn_id = message
        .pointer("/params/turn/id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(turn_id) = turn_id else { return };
    match observe_started_turn(&session.active_turn, turn_id) {
        StartedTurnDisposition::Adopted => {
            session.set_status("working", Some(turn_id.to_owned()));
        }
        StartedTurnDisposition::Matched => {}
        StartedTurnDisposition::Conflict => {
            tracing::warn!(
                group_id = %session.group_id,
                actor_id = %session.actor_id,
                turn_id,
                "managed Actor reported an overlapping terminal turn; stopping the inconsistent session"
            );
            let _ = session.stop();
        }
    }
}

fn observe_started_turn(
    active_turn: &std::sync::Mutex<Option<ActiveTurn>>,
    turn_id: &str,
) -> StartedTurnDisposition {
    let Ok(mut active_turn) = active_turn.lock() else {
        return StartedTurnDisposition::Conflict;
    };
    match active_turn.as_mut() {
        Some(active) if active.turn_id == turn_id => StartedTurnDisposition::Matched,
        Some(_) => StartedTurnDisposition::Conflict,
        None => {
            *active_turn = Some(ActiveTurn {
                turn_id: turn_id.to_owned(),
                started_at: cccc_contracts::utc_now(),
            });
            StartedTurnDisposition::Adopted
        }
    }
}

/// A lost provider stream is a failed handoff, not a completed user task.
/// Reuse normal report promotion/deduplication; an idle or intentionally stopped
/// session has no unfinished turn to report.
pub(super) fn fail_active_turn(session: &Session, reason: &str) {
    if session.stopped.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let Some(turn_id) = active_context(session) else {
        return;
    };
    complete_turn(
        session,
        &json!({"params":{"turn":{
            "id":turn_id,"status":"failed","error":{"message":reason}
        }}}),
    );
}

fn complete_turn(session: &Session, message: &Value) {
    let Ok(mut active_turn) = session.active_turn.lock() else {
        return;
    };
    let Some(current) = active_turn.as_ref() else {
        return;
    };
    let reported_turn_id = message
        .pointer("/params/turn/id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !reported_turn_id.is_empty()
        && !current.turn_id.is_empty()
        && reported_turn_id != current.turn_id
    {
        return;
    }
    let finished = active_turn.take().expect("matched active turn");
    drop(active_turn);
    session.set_status("idle", None);
    if let Err(error) = notify_web_foreman(
        &session.home,
        &session.group_id,
        &session.actor_id,
        &finished.turn_id,
        &finished.started_at,
        message,
    ) {
        tracing::error!(%error, group_id=%session.group_id, actor_id=%session.actor_id,
            turn_id=%finished.turn_id, "failed to record local member completion notice");
    }
}

// Completion is a handoff fact, never a decision that the overall task is done.
// Reuse the existing ledger, send operation and idempotency key; no new loop.
pub(crate) fn notify_web_foreman(
    home: &cccc_core::HomeLayout,
    group_id: &str,
    actor_id: &str,
    turn_id: &str,
    started_at: &str,
    message: &Value,
) -> std::io::Result<bool> {
    use cccc_contracts::{ActorRuntime, DaemonRequest, GroupState};
    use cccc_core::{GroupStore, actors, ledger};
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    let delivery_enabled =
        group.running && !matches!(group.state, GroupState::Paused | GroupState::Stopped);
    let Ok(lead) = actors::unique_available_foreman(&group) else {
        return Ok(false);
    };
    if lead.runtime != ActorRuntime::WebModel || lead.id == actor_id {
        return Ok(false);
    }
    let status = message
        .pointer("/params/turn/status")
        .and_then(Value::as_str)
        .unwrap_or("ended");
    let source_event_id = message
        .pointer("/params/turn/source_event_id")
        .and_then(Value::as_str);
    // ponytail: bounded scan may produce an extra reminder on extremely busy groups;
    // use indexed turn ownership if the group exceeds 2,000 messages per member turn.
    let (recent, _) =
        ledger::tail_filtered(&store.ledger_path(group_id)?, 2_000, Some("chat.message"))?;
    let mut reports = recent
        .iter()
        .filter(|event| {
            event.by == actor_id
                && event.ts.as_str() >= started_at
                && source_event_id
                    .is_none_or(|id| event.data.get("reply_to").and_then(Value::as_str) == Some(id))
                && event
                    .data
                    .get("to")
                    .and_then(Value::as_array)
                    .is_some_and(|to| to.iter().any(|id| id.as_str() == Some(lead.id.as_str())))
        })
        .cloned()
        .collect::<Vec<_>>();
    if reports.is_empty() || (status == "failed" && source_event_id.is_some()) {
        if !delivery_enabled {
            return Ok(false);
        }
        let key = super::super::message_idempotency::tracked_client_id(
            group_id,
            actor_id,
            &format!("completion:{turn_id}"),
        );
        let detail = source_event_id
            .map(|id| {
                format!(
                    " Source event: {id}. {}",
                    message
                        .pointer("/params/turn/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                )
            })
            .unwrap_or_default();
        let mut request = DaemonRequest { v:1, op:"send".into(), args:json!({
            "group_id":group_id,"by":"system","to":[lead.id],"message_mode":"send","client_id":key,
            "text":format!("[CCCC] Member {actor_id} ended this turn ({status}).{detail} Please review its actual result and decide the next step; this notice does not mark the task complete."),
            "source_actor_id":actor_id,"source_turn_id":turn_id
        }).as_object().cloned().expect("completion request") };
        if let Some(source) = source_event_id {
            request.args.insert("reply_to".into(), json!(source));
        }
        let sent = super::super::messaging::send(home, &request, "chat.message")
            .map_err(|error| std::io::Error::other(format!("{}: {}", error.code, error.message)))?;
        let event: cccc_contracts::Event =
            serde_json::from_value(sent.get("event").cloned().unwrap_or(Value::Null))
                .map_err(std::io::Error::other)?;
        reports.push(event);
    }
    if delivery_enabled {
        for report in &reports {
            if report.data.get("message_mode").and_then(Value::as_str) != Some("mail") {
                continue;
            }
            // Reuse every visible report. Mail alone cannot wake an idle browser
            // Foreman; native delivery promotion preserves both message and cursor.
            let request=DaemonRequest {v:1,op:"message_deliver".into(),args:json!({
                "group_id":group_id,"by":actor_id,"source_event_id":report.id,"actor_ids":[lead.id]
            }).as_object().cloned().expect("report delivery request")};
            let result = super::super::messaging::resolve_operation(&request)
                .expect("native message delivery")
                .execute(home, &request);
            if let Err(error) = result {
                if !matches!(
                    error.code.as_str(),
                    "already_delivered" | "delivery_in_progress" | "delivery_ambiguous"
                ) {
                    return Err(std::io::Error::other(format!(
                        "{}: {}",
                        error.code, error.message
                    )));
                }
            }
        }
    }
    super::super::coordination_relay::record_handoff(
        home, &group, actor_id, &lead.id, turn_id, &reports, status,
    )
    .map(|_| true)
    .map_err(|error| std::io::Error::other(format!("{}: {}", error.code, error.message)))
}

fn active_context(session: &Session) -> Option<String> {
    session
        .active_turn
        .lock()
        .ok()?
        .as_ref()
        .map(|turn| turn.turn_id.clone())
}

pub(super) fn emit(session: &Session, kind: &str, data: Map<String, Value>) {
    if let Err(error) = events::append(
        &session.home,
        &session.group_id,
        &session.actor_id,
        kind,
        data,
    ) {
        tracing::warn!(%error, group_id = %session.group_id, actor_id = %session.actor_id, "failed to append headless event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untracked_codex_turn_is_adopted_until_its_completion() {
        let active_turn = std::sync::Mutex::new(None);

        assert_eq!(
            observe_started_turn(&active_turn, "turn-terminal"),
            StartedTurnDisposition::Adopted
        );
        let active = active_turn.lock().expect("active turn");
        let active = active.as_ref().expect("adopted turn");
        assert_eq!(active.turn_id, "turn-terminal");
    }

    #[test]
    fn a_repeated_started_event_matches_the_active_turn_but_not_an_overlap() {
        let active_turn = std::sync::Mutex::new(Some(ActiveTurn {
            turn_id: "turn-terminal".into(),
            started_at: cccc_contracts::utc_now(),
        }));

        assert_eq!(
            observe_started_turn(&active_turn, "turn-terminal"),
            StartedTurnDisposition::Matched
        );
        assert_eq!(
            observe_started_turn(&active_turn, "turn-overlap"),
            StartedTurnDisposition::Conflict
        );
    }
    #[test]
    fn completed_turns_keep_original_reports_and_promote_only_unpaused_mail() {
        use cccc_contracts::{Actor, ActorRuntime, Event, GroupState};
        use cccc_core::{GroupStore, HomeLayout, actors, ledger};

        for (paused, modes) in [
            (false, &[][..]),
            (false, &["send"][..]),
            (false, &["mail"][..]),
            (false, &["mail", "send"][..]),
            (true, &[][..]),
            (true, &["mail"][..]),
        ] {
            let temp = tempfile::tempdir().expect("isolated turn");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = GroupStore::new(home.clone()).expect("store");
            let mut group = store.create("completion", "").expect("group");
            let mut lead = Actor::new("web-lead");
            lead.runtime = ActorRuntime::WebModel;
            actors::add(&mut group, lead).expect("lead");
            actors::add(&mut group, Actor::new("worker")).expect("worker");
            group.running = true;
            if paused {
                group.state = GroupState::Paused;
            }
            store.save(&group).expect("save");
            let path = store.ledger_path(&group.group_id).expect("ledger");
            let mut old = Event::new("chat.message", &group.group_id);
            old.by = "worker".into();
            old.ts = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
            old.data = json!({"to":["web-lead"],"message_mode":"mail","text":"Earlier turn"})
                .as_object()
                .cloned()
                .expect("old report");
            ledger::append(&path, &old).expect("old turn output");
            let turn = ActiveTurn {
                turn_id: "current-turn".into(),
                started_at: cccc_contracts::utc_now(),
            };
            let reports = modes
                .iter()
                .map(|mode| {
                    let mut event = Event::new("chat.message", &group.group_id);
                    event.by = "worker".into();
                    event.data =
                        json!({"to":["web-lead"],"message_mode":mode,"text":"Actual result"})
                            .as_object()
                            .cloned()
                            .expect("report");
                    ledger::append(&path, &event).expect("original report");
                    event
                })
                .collect::<Vec<_>>();
            let completed = json!({"params":{"turn":{"id":turn.turn_id,"status":"completed"}}});
            for _ in 0..2 {
                notify_web_foreman(
                    &home,
                    &group.group_id,
                    "worker",
                    &turn.turn_id,
                    &turn.started_at,
                    &completed,
                )
                .expect("completion is repeatable");
            }
            let events = ledger::read_all(&path).expect("ledger");
            let messages = events
                .iter()
                .filter(|event| event.kind == "chat.message")
                .collect::<Vec<_>>();
            let handoffs = events
                .iter()
                .filter(|event| event.kind == "coordination.handoff")
                .collect::<Vec<_>>();
            let fallback = reports.is_empty() && !paused;
            assert_eq!(
                messages.len(),
                1 + reports.len() + usize::from(fallback),
                "output replaced or duplicated: paused={paused}, modes={modes:?}"
            );
            let expected_handoffs = usize::from(fallback || !reports.is_empty());
            assert_eq!(handoffs.len(), expected_handoffs, "one handoff per turn");
            if expected_handoffs == 1 {
                let source_ids = if fallback {
                    let notice = messages
                        .iter()
                        .find(|event| event.by == "system")
                        .expect("fallback notice");
                    assert_eq!(notice.data["to"], json!(["web-lead"]));
                    assert_eq!(notice.data["message_mode"], "send");
                    vec![notice.id.clone()]
                } else {
                    reports.iter().map(|report| report.id.clone()).collect()
                };
                assert_eq!(handoffs[0].data["source_event_ids"], json!(source_ids));
                assert_eq!(handoffs[0].data["target_actor_id"], "web-lead");
            }
            for report in &reports {
                assert!(
                    messages
                        .iter()
                        .any(|event| event.id == report.id && event.data == report.data),
                    "original report changed"
                );
                let promoted = events.iter().any(|event| {
                    event.kind == "runtime.delivery" && event.data["source_event_id"] == report.id
                });
                assert_eq!(
                    promoted,
                    !paused && report.data["message_mode"] == "mail",
                    "wrong promotion: paused={paused}, modes={modes:?}"
                );
            }
            assert!(
                !events.iter().any(|event| event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == old.id),
                "earlier turn was promoted"
            );
            let context = cccc_core::context::ContextStore::new(home.clone())
                .expect("context store")
                .load(&group.group_id)
                .expect("context");
            assert_eq!(
                context
                    .coordination
                    .get("recent_handoffs")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len),
                expected_handoffs,
                "context and ledger disagree"
            );
        }
    }
}

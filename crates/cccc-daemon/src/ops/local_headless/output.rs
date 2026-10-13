use super::{ActiveTurn, Session, events};
use serde_json::{Map, Value, json};

pub(super) fn handle_message(session: &Session, message: Value) {
    if message.get("id").is_some() {
        if message.get("method").and_then(Value::as_str).is_some() {
            respond_unsupported_server_request(session, &message);
        }
        return;
    }
    if session.managed.structured_only() {
        project_structured_event(session, &message);
    }
    handle_announced_message(session, message);
}

// Reuse the existing headless journal and frontend projection. Store only the
// normalized, selected event fields, never opaque protocol/configuration frames.
fn project_structured_event(session: &Session, message: &Value) {
    let params = &message["params"];
    let turn_id = params["turnId"]
        .as_str()
        .or_else(|| params.pointer("/turn/id").and_then(Value::as_str))
        .unwrap_or_default();
    let stream_id = format!("{}:{turn_id}", session.managed.generation());
    let mut data = Map::from_iter([
        ("turn_id".into(), json!(turn_id)),
        ("stream_id".into(), json!(stream_id)),
    ]);
    let kind = match message["method"].as_str().unwrap_or_default() {
        "turn/started" => "headless.turn.started",
        "turn/completed" => {
            emit(session, "headless.message.completed", data.clone());
            data.insert("status".into(), params["turn"]["status"].clone());
            if params["turn"]["status"] == "failed" {
                data.insert("error".into(), params["turn"]["error"].clone());
                "headless.turn.failed"
            } else {
                "headless.turn.completed"
            }
        }
        "item/agentMessage/delta" => {
            data.insert("delta".into(), params["delta"].clone());
            "headless.message.delta"
        }
        "cccc/approvalRequired" => {
            data.insert("summary".into(), json!("Permission confirmation required"));
            data.insert("kind".into(), json!("approval"));
            "headless.activity.updated"
        }
        _ => return,
    };
    emit(session, kind, data);
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
    // Codex sub-agents run on their own threads and announce their own turns.
    // Those are not this Actor's terminal turn; treating one as an overlap
    // would stop a healthy session the moment the model delegates work.
    if is_foreign_thread(session.managed.thread_id(), &message) {
        return;
    }
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

/// True when the notification names a thread other than the session's own.
/// Notifications without a `threadId` (non-Codex providers, lifecycle
/// messages) always belong to the session.
fn is_foreign_thread(session_thread_id: &str, message: &Value) -> bool {
    !session_thread_id.is_empty()
        && message
            .pointer("/params/threadId")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|thread_id| !thread_id.is_empty() && thread_id != session_thread_id)
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
            });
            StartedTurnDisposition::Adopted
        }
    }
}

fn complete_turn(session: &Session, message: &Value) {
    let Ok(mut active_turn) = session.active_turn.lock() else {
        return;
    };
    let Some(current) = active_turn.as_ref() else {
        return;
    };
    let turn_id = current.turn_id.clone();
    let Some(settled) = settle_decision(&turn_id, message) else {
        // A completion for some other turn is not this session's to settle.
        return;
    };
    active_turn.take();
    // A completed turn is not automatically a successful turn. Providers report
    // failure out of band (ACP stopReason, a model API error surfaced as an
    // errored assistant message), and reporting "idle" for a rejected turn hid
    // a broken lane behind a healthy one. Classify before settling.
    //
    // Cancellation is deliberately NOT a failure. It is usually an operator
    // interrupt, a cccc cancel_turn, or the owner's Esc, and marking a
    // deliberate stop as an error would train every reader to ignore the
    // error state. It stays visible in history without claiming breakage.
    match settled.outcome {
        TurnSettle::Failed(reason) => {
            emit(
                session,
                "headless.turn.failed",
                Map::from_iter([
                    ("turn_id".into(), json!(turn_id)),
                    ("status".into(), json!(settled.status)),
                    ("error".into(), json!(reason)),
                ]),
            );
            session.set_status_with_reason("error", None, Some(reason));
        }
        TurnSettle::Cancelled => {
            emit(
                session,
                "headless.turn.cancelled",
                Map::from_iter([
                    ("turn_id".into(), json!(turn_id)),
                    ("status".into(), json!(settled.status)),
                ]),
            );
            session.set_status("idle", None);
        }
        TurnSettle::Completed => session.set_status("idle", None),
    }
}

/// How a turn completion settles.
#[derive(Debug)]
enum TurnSettle {
    Completed,
    Cancelled,
    Failed(String),
}

/// Decide how a turn completion settles. `None` means the completion names a
/// different turn and must be ignored; this is the guard that existed before
/// failure classification and must keep applying to both outcomes.
fn settle_decision(active_turn_id: &str, message: &Value) -> Option<TurnOutcome> {
    let reported_turn_id = message
        .pointer("/params/turn/id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !reported_turn_id.is_empty()
        && !active_turn_id.is_empty()
        && reported_turn_id != active_turn_id
    {
        return None;
    }
    Some(turn_outcome(message))
}

/// Normalise a turn completion into (status, error).
///
/// Claude (`claude/transcript.rs`) and the ACP adapters
/// (`acp/events.rs::settle_turn`) both publish `params.turn.status` plus
/// `params.turn.error`. Codex republishes the provider frame unchanged, so a
/// missing status means the provider reported no outcome rather than success.
/// `error` may be a string or an object; normalise both to text.
fn turn_outcome(message: &Value) -> TurnOutcome {
    let status = message
        .pointer("/params/turn/status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("completed")
        .to_ascii_lowercase();
    let error = message
        .pointer("/params/turn/error")
        .filter(|error| !error.is_null())
        .map(error_text);
    let outcome = settle_outcome(&status, error);
    TurnOutcome { status, outcome }
}

fn error_text(error: &Value) -> String {
    match error {
        Value::String(text) if !text.trim().is_empty() => text.trim().to_owned(),
        Value::String(_) => "turn failed without a reported reason".to_owned(),
        Value::Object(_) => ["message", "error", "detail", "reason"]
            .into_iter()
            .find_map(|field| {
                error
                    .get(field)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| error.to_string()),
        other => other.to_string(),
    }
}

struct TurnOutcome {
    status: String,
    outcome: TurnSettle,
}

fn settle_outcome(status: &str, error: Option<String>) -> TurnSettle {
    // A cancellation is a deliberate stop, not a fault. `acp/events.rs` and
    // `claude/transcript.rs` both publish "cancelled" for an operator
    // interrupt, so it must not read as an error.
    if matches!(status, "cancelled" | "canceled" | "interrupted") {
        return TurnSettle::Cancelled;
    }
    if status == "completed" && error.is_none() {
        return TurnSettle::Completed;
    }
    TurnSettle::Failed(error.unwrap_or_else(|| match status {
        "failed" => "provider reported the turn failed without a reason".to_owned(),
        other => format!("turn ended with status {other}"),
    }))
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
    fn only_turns_on_other_threads_are_foreign() {
        let sub_agent = json!({
            "method":"turn/started",
            "params":{"threadId":"thread-sub-agent","turn":{"id":"turn-sub"}}
        });
        let own = json!({
            "method":"turn/started",
            "params":{"threadId":"thread-main","turn":{"id":"turn-main"}}
        });
        let untagged = json!({"method":"turn/completed","params":{"turn":{"id":"turn-main"}}});

        assert!(is_foreign_thread("thread-main", &sub_agent));
        assert!(!is_foreign_thread("thread-main", &own));
        assert!(!is_foreign_thread("thread-main", &untagged));
        // A session without a known thread cannot tell threads apart.
        assert!(!is_foreign_thread("", &sub_agent));
    }

    #[test]
    fn a_repeated_started_event_matches_the_active_turn_but_not_an_overlap() {
        let active_turn = std::sync::Mutex::new(Some(ActiveTurn {
            turn_id: "turn-terminal".into(),
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
    /// Exercise the status/reason round trip on a live `Session`, not just the
    /// classifier. A failed turn must leave status error with the provider's
    /// reason, and the next successful turn must clear both status and reason
    /// back to idle. The fake control socket is Unix-only.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_turn_clears_on_the_next_success_through_a_real_session() {
        use super::super::HeadlessStatus;
        use crate::ops::codex_voice_analyst::AnalystSession;
        use cccc_core::HomeLayout;
        use std::sync::{Arc, Mutex, atomic::AtomicBool};

        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().canonicalize().expect("config");
        let (_listener, _directory) =
            crate::ops::local_headless::supervisor::control_fixture::bind(&config);
        let home = HomeLayout::from_path(config.join("home")).expect("home");
        let status = Mutex::new(HeadlessStatus {
            status: "working".into(),
            task_id: None,
            updated_at: String::new(),
            pid: None,
            reason: None,
        });
        let session = Session {
            home,
            group_id: "g-session-roundtrip".into(),
            actor_id: "claude-1".into(),
            managed: Arc::new(AnalystSession::claude_for_shutdown_test(
                &config,
                "abcdef01",
                Vec::new(),
                true,
            )),
            has_terminal: AtomicBool::new(false),
            viewer: Mutex::new(None),
            status,
            stopped: AtomicBool::new(false),
            stop_lock: Mutex::new(()),
            startup_prompt: Mutex::new(None),
            active_turn: Mutex::new(None),
        };

        // A rejected turn: status error carrying the provider's own text.
        session.set_status_with_reason(
            "error",
            None,
            Some("APIError 400 reasoning encrypted_content".into()),
        );
        assert_eq!(session.status.lock().expect("status").status, "error");
        assert_eq!(
            session.status.lock().expect("status").reason.as_deref(),
            Some("APIError 400 reasoning encrypted_content")
        );

        // The next successful turn must clear the stale failure entirely.
        session.set_status("idle", None);
        let state = session.status.lock().expect("status");
        assert_eq!(state.status, "idle");
        assert_eq!(
            state.reason, None,
            "a recovered turn must not keep displaying the previous failure"
        );
    }

    /// Classify a completion the way `complete_turn` does, including the
    /// turn-id guard. `None` means the turn was ignored; otherwise the settle
    /// decision, with the reported status for context.
    fn classify(active_turn_id: &str, message: Value) -> Option<(TurnSettle, String)> {
        let outcome = settle_decision(active_turn_id, &message)?;
        Some((outcome.outcome, outcome.status))
    }

    fn failed_reason(settled: &(TurnSettle, String)) -> &str {
        match &settled.0 {
            TurnSettle::Failed(reason) => reason,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    // Claude publishes status+error via claude/transcript.rs settle_turn.
    // ACP (OpenCode/Kilo) publishes the same shape via acp/events.rs settle_turn.
    // Codex republishes the provider frame unchanged.
    const CLAUDE_FAILED: &str = r#"{"method":"turn/completed","params":{"threadId":"t1","turn":{"id":"turn-1","status":"failed","error":"ACP turn stopped: provider_error"}}}"#;
    const ACP_OPENCODE_FAILED: &str = r#"{"method":"turn/completed","params":{"threadId":"t1","turn":{"id":"turn-1","status":"failed","error":"ACP turn stopped: max_tokens"}}}"#;
    const CODEX_FAILED: &str = r#"{"method":"turn/completed","params":{"threadId":"t1","turn":{"id":"turn-1","status":"failed","error":{"message":"reasoning encrypted_content was not issued to this caller","code":"api_error"}}}}"#;

    #[test]
    fn a_failed_completion_is_reported_as_a_failure_with_its_reason() {
        for (runtime, fixture, expected) in [
            ("claude", CLAUDE_FAILED, "ACP turn stopped"),
            ("opencode", ACP_OPENCODE_FAILED, "ACP turn stopped"),
            ("codex", CODEX_FAILED, "encrypted_content"),
        ] {
            let settled = classify("turn-1", json!(fixture_parse(fixture))).expect("settled");
            assert!(matches!(settled.0, TurnSettle::Failed(_)), "{runtime}");
            assert!(
                failed_reason(&settled).contains(expected),
                "{runtime} reason was {:?}",
                failed_reason(&settled)
            );
        }
    }

    /// A deliberate cancel is not a fault. It settles idle, never error, so the
    /// error state keeps meaning "this lane is broken".
    #[test]
    fn a_cancelled_completion_settles_idle_and_is_not_an_error() {
        for status in ["cancelled", "canceled", "interrupted"] {
            let fixture = json!({
                "method": "turn/completed",
                "params": {"turn": {"id": "turn-1", "status": status, "error": null}}
            });
            let settled = classify("turn-1", fixture).expect("settled");
            assert!(
                matches!(settled.0, TurnSettle::Cancelled),
                "{status} must not settle as a failure, got {:?}",
                settled.0
            );
        }
        // A cancellation carrying an error text is still a cancellation.
        let settled = classify(
            "turn-1",
            json!(fixture_parse(
                r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"cancelled","error":"operator interrupt"}}}"#
            )),
        )
        .expect("settled");
        assert!(matches!(settled.0, TurnSettle::Cancelled));
    }

    #[test]
    fn a_successful_completion_settles_idle() {
        for fixture in [
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"completed","error":null}}}"#,
            // A provider that reports no status at all is not claiming failure.
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1"}}}"#,
        ] {
            let settled = classify("turn-1", json!(fixture_parse(fixture))).expect("settled");
            assert!(
                matches!(settled.0, TurnSettle::Completed),
                "expected completed, got {:?}",
                settled.0
            );
        }
    }

    /// "completed" plus an error is a contradiction; trust the error rather
    /// than reporting a turn that carried a failure as a success.
    #[test]
    fn a_completed_status_carrying_an_error_still_fails() {
        let settled = classify(
            "turn-1",
            json!(fixture_parse(
                r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"completed","error":"partial failure"}}}"#
            )),
        )
        .expect("settled");
        assert!(matches!(settled.0, TurnSettle::Failed(_)));
        assert_eq!(failed_reason(&settled), "partial failure");
    }

    #[test]
    fn a_failed_turn_that_reports_no_reason_still_fails_loudly() {
        let settled = classify(
            "turn-1",
            json!(fixture_parse(
                r#"{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"failed"}}}"#
            )),
        )
        .expect("settled");
        assert_eq!(
            failed_reason(&settled),
            "provider reported the turn failed without a reason"
        );
    }

    #[test]
    fn a_completion_for_another_turn_is_ignored_whatever_its_outcome() {
        for fixture in [
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-other","status":"failed","error":"wrong turn"}}}"#,
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-other","status":"completed"}}}"#,
            r#"{"method":"turn/completed","params":{"turn":{"id":"turn-other","status":"cancelled"}}}"#,
        ] {
            assert!(classify("turn-main", json!(fixture_parse(fixture))).is_none());
        }
    }

    /// A failed turn must not leave the session permanently in error: the next
    /// successful completion settles it back to idle.
    #[test]
    fn a_successful_turn_after_a_failed_one_clears_the_error() {
        let failed = classify("turn-1", json!(fixture_parse(CODEX_FAILED))).expect("settled");
        assert!(matches!(failed.0, TurnSettle::Failed(_)));
        let recovered = classify(
            "turn-2",
            json!(fixture_parse(
                r#"{"method":"turn/completed","params":{"turn":{"id":"turn-2","status":"completed"}}}"#
            )),
        )
        .expect("settled");
        assert!(
            matches!(recovered.0, TurnSettle::Completed),
            "the next successful turn must settle idle, clearing the stale reason"
        );
    }

    fn fixture_parse(fixture: &str) -> Value {
        serde_json::from_str(fixture.trim()).expect("fixture json")
    }
}

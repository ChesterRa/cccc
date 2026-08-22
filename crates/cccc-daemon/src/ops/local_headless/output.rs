use super::{ActiveTurn, Session, Turn, TurnOutputState, events};
use cccc_contracts::ActorRuntime;
use serde_json::{Map, Value, json};

pub(super) fn handle_message(session: &Session, message: Value) {
    if message.get("id").is_some() {
        if message.get("method").and_then(Value::as_str).is_some() {
            respond_unsupported_server_request(session, &message);
        } else if let Some(id) = message.get("id").and_then(Value::as_u64)
            && let Some(sender) = session
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id))
        {
            let _ = sender.try_send(message);
        }
        return;
    }
    if defer_until_announced(session, &message) {
        return;
    }
    handle_announced_message(session, message);
}

pub(super) fn announce_turn(session: &Session) {
    drain_buffered_messages(&session.active_turn, |message| {
        handle_announced_message(session, message);
    });
}

fn defer_until_announced(session: &Session, message: &Value) -> bool {
    defer_message_until_announced(&session.active_turn, message)
}

fn defer_message_until_announced(
    active_turn: &std::sync::Mutex<Option<ActiveTurn>>,
    message: &Value,
) -> bool {
    let Ok(mut active_turn) = active_turn.lock() else {
        return false;
    };
    let Some(active_turn) = active_turn.as_mut() else {
        return false;
    };
    if active_turn.output_state == TurnOutputState::Announced {
        return false;
    }
    active_turn.pending_messages.push(message.clone());
    true
}

fn drain_buffered_messages(
    active_turn: &std::sync::Mutex<Option<ActiveTurn>>,
    mut handle: impl FnMut(Value),
) {
    loop {
        let pending_messages = {
            let Ok(mut active_turn) = active_turn.lock() else {
                return;
            };
            let Some(active_turn) = active_turn.as_mut() else {
                return;
            };
            active_turn.output_state = TurnOutputState::Draining;
            if active_turn.pending_messages.is_empty() {
                active_turn.output_state = TurnOutputState::Announced;
                return;
            }
            std::mem::take(&mut active_turn.pending_messages)
        };
        for message in pending_messages {
            handle(message);
        }
    }
}

fn respond_unsupported_server_request(session: &Session, message: &Value) {
    let Some(id) = message.get("id") else {
        return;
    };
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let _ = session.write_json(&json!({
        "jsonrpc":"2.0",
        "id":id,
        "error":{
            "code":-32601,
            "message":format!("CCCC headless does not support provider request: {method}")
        }
    }));
}

fn handle_announced_message(session: &Session, message: Value) {
    let completed = if session.runtime == ActorRuntime::Codex {
        message.get("method").and_then(Value::as_str) == Some("turn/completed")
    } else {
        message.get("type").and_then(Value::as_str) == Some("result")
    };
    if completed {
        complete_turn(session, &message);
        return;
    }
    if session.runtime == ActorRuntime::Codex {
        handle_codex_output(session, &message);
    } else {
        handle_claude_output(session, &message);
    }
    if session.runtime == ActorRuntime::Codex
        && message.get("method").and_then(Value::as_str) == Some("thread/status/changed")
    {
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
        let task = active_context(session).map(|(_, turn_id, _)| turn_id);
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

fn complete_turn(session: &Session, message: &Value) {
    let Ok(mut active_turn) = session.active_turn.lock() else {
        return;
    };
    let Some(current) = active_turn.as_ref() else {
        return;
    };
    if session.runtime == ActorRuntime::Codex {
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
    }
    let Some(current) = active_turn.take() else {
        return;
    };
    drop(active_turn);

    session.set_status("idle", None);
    if let Ok(mut generation) = session.completion.0.lock() {
        *generation += 1;
    }
    session.completion.1.notify_all();
    let (kind, mut data) = if session.runtime == ActorRuntime::Codex {
        codex_terminal_event(message, &current.event_id)
    } else {
        claude_terminal_event(message, &current.event_id)
    };
    let resume_rejection = if session.runtime == ActorRuntime::Claude {
        claude_resume_rejection(session, message)
    } else {
        None
    };
    data.entry("turn_id")
        .or_insert_with(|| json!(current.turn_id));
    let is_control = !current.control_kind.is_empty();
    if is_control {
        data.insert("control_kind".into(), json!(current.control_kind));
    }
    let control_kind = is_control.then(|| kind.replace("turn", "control"));
    emit(session, control_kind.as_deref().unwrap_or(kind), data);
    if let Some(error) = resume_rejection {
        session.stop_after_invalidate(|| {
            super::session::invalidate_pending_claude_resume(session, &error);
        });
    }
}

fn claude_resume_rejection(session: &Session, message: &Value) -> Option<String> {
    let subtype = message
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let normalized_subtype = subtype.trim().to_ascii_lowercase();
    let failed = message.get("is_error").and_then(Value::as_bool) == Some(true)
        || normalized_subtype == "error"
        || normalized_subtype.starts_with("error_");
    if !failed {
        if let Ok(mut session_id) = session.resumed_provider_session_id.lock() {
            session_id.clear();
        }
        return None;
    }
    let error = claude_result_error(message, subtype)
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(subtype)
        .trim()
        .to_owned();
    super::super::runtime_session::resume_failure_marker(&error)?;
    let has_pending_resume = session
        .resumed_provider_session_id
        .lock()
        .ok()
        .is_some_and(|session_id| !session_id.is_empty());
    if !has_pending_resume {
        return None;
    }
    Some(error)
}

fn active_context(session: &Session) -> Option<(String, String, String)> {
    session.active_turn.lock().ok()?.as_ref().map(|turn| {
        (
            turn.event_id.clone(),
            turn.turn_id.clone(),
            turn.control_kind.clone(),
        )
    })
}

fn codex_terminal_event(message: &Value, event_id: &str) -> (&'static str, Map<String, Value>) {
    let turn = message.pointer("/params/turn").and_then(Value::as_object);
    let turn_id = turn
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = turn
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let error = turn
        .and_then(|value| value.get("error"))
        .filter(|value| !value.is_null())
        .map(normalize_provider_error);
    let failed = matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "failed" | "error" | "cancelled"
    ) || error.is_some();
    (
        if failed {
            "headless.turn.failed"
        } else {
            "headless.turn.completed"
        },
        Map::from_iter([
            ("turn_id".into(), json!(turn_id)),
            ("event_id".into(), json!(event_id)),
            ("status".into(), json!(status)),
            ("error".into(), error.unwrap_or(Value::Null)),
        ]),
    )
}

fn claude_terminal_event(message: &Value, event_id: &str) -> (&'static str, Map<String, Value>) {
    let subtype = message
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let normalized_subtype = subtype.trim().to_ascii_lowercase();
    let failed = message.get("is_error").and_then(Value::as_bool) == Some(true)
        || normalized_subtype == "error"
        || normalized_subtype.starts_with("error_");
    let status = if subtype.trim().is_empty() {
        "completed"
    } else {
        subtype
    };
    let error = failed.then(|| claude_result_error(message, status));
    (
        if failed {
            "headless.turn.failed"
        } else {
            "headless.turn.completed"
        },
        Map::from_iter([
            ("event_id".into(), json!(event_id)),
            ("status".into(), json!(status)),
            ("error".into(), error.unwrap_or(Value::Null)),
        ]),
    )
}

fn claude_result_error(message: &Value, status: &str) -> Value {
    if let Some(error) = message.get("error").filter(|value| !value.is_null()) {
        return normalize_provider_error(error);
    }
    if let Some(result) = message.get("result").filter(|value| !value.is_null()) {
        if result.as_str().is_none_or(|value| !value.trim().is_empty()) {
            return normalize_provider_error(result);
        }
    }
    if let Some(errors) = message.get("errors").and_then(Value::as_array) {
        let messages = errors
            .iter()
            .filter_map(|value| {
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .collect::<Vec<_>>();
        if !messages.is_empty() {
            return json!({"message":messages.join("; ")});
        }
    }
    json!({"message":if status.is_empty() { "Claude provider result failed" } else { status }})
}

fn normalize_provider_error(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else if let Some(message) = value.as_str() {
        json!({"message":message})
    } else {
        json!({"message":value.to_string()})
    }
}

fn handle_codex_output(session: &Session, message: &Value) {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if method == "item/agentMessage/delta" {
        emit_message(
            session,
            "headless.message.delta",
            message
                .pointer("/params/delta")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            message
                .pointer("/params/itemId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    } else if method == "item/completed"
        && message.pointer("/params/item/type").and_then(Value::as_str) == Some("agentMessage")
    {
        emit_message(
            session,
            "headless.message.completed",
            message
                .pointer("/params/item/text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            message
                .pointer("/params/item/id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
}

fn handle_claude_output(session: &Session, message: &Value) {
    let kind = message
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "stream_event"
        && message.pointer("/event/type").and_then(Value::as_str) == Some("content_block_delta")
    {
        emit_message(
            session,
            "headless.message.delta",
            message
                .pointer("/event/delta/text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            message
                .get("uuid")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    } else if kind == "assistant" {
        let text = message
            .pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        emit_message(
            session,
            "headless.message.completed",
            &text,
            message
                .pointer("/message/id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
}

fn emit_message(session: &Session, kind: &str, text: &str, stream_id: &str) {
    if text.is_empty() {
        return;
    }
    let Some((event_id, turn_id, _)) = active_context(session) else {
        return;
    };
    let key = if kind.ends_with("delta") {
        "delta"
    } else {
        "text"
    };
    emit(
        session,
        kind,
        Map::from_iter([
            ("turn_id".into(), json!(turn_id)),
            ("event_id".into(), json!(event_id)),
            ("stream_id".into(), json!(stream_id)),
            (key.into(), json!(text)),
        ]),
    );
}

pub(super) fn emit_turn(session: &Session, turn: &Turn, kind: &str, turn_id: &str) {
    let control_event_kind =
        (!turn.control_kind.is_empty()).then(|| kind.replace("turn", "control"));
    let mut data = Map::from_iter([
        ("turn_id".into(), json!(turn_id)),
        ("event_id".into(), json!(turn.event_id)),
    ]);
    if !turn.control_kind.is_empty() {
        data.insert("control_kind".into(), json!(turn.control_kind));
    }
    emit(session, control_event_kind.as_deref().unwrap_or(kind), data);
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
    fn output_arriving_during_backlog_drain_stays_ordered() {
        let active_turn = std::sync::Mutex::new(Some(ActiveTurn {
            event_id: "event-1".into(),
            turn_id: "turn-1".into(),
            control_kind: String::new(),
            output_state: TurnOutputState::Buffering,
            pending_messages: vec![json!({"sequence":1})],
        }));
        let mut observed = Vec::new();

        drain_buffered_messages(&active_turn, |message| {
            let sequence = message["sequence"].as_u64().expect("sequence");
            observed.push(sequence);
            if sequence == 1 {
                assert!(defer_message_until_announced(
                    &active_turn,
                    &json!({"sequence":2})
                ));
            }
        });

        assert_eq!(observed, vec![1, 2]);
        assert_eq!(
            active_turn
                .lock()
                .expect("active turn")
                .as_ref()
                .expect("turn")
                .output_state,
            TurnOutputState::Announced
        );
    }

    #[test]
    fn codex_failed_completion_preserves_status_and_error() {
        let message = json!({
            "method":"turn/completed",
            "params":{"turn":{
                "id":"turn-failed",
                "status":"failed",
                "error":{"message":"provider failed"}
            }}
        });

        let (kind, data) = codex_terminal_event(&message, "event-1");

        assert_eq!(kind, "headless.turn.failed");
        assert_eq!(data["turn_id"], "turn-failed");
        assert_eq!(data["event_id"], "event-1");
        assert_eq!(data["status"], "failed");
        assert_eq!(data["error"]["message"], "provider failed");
    }

    #[test]
    fn codex_cancelled_or_explicit_error_is_not_reported_completed() {
        for message in [
            json!({"params":{"turn":{"id":"turn-cancelled","status":"cancelled"}}}),
            json!({"params":{"turn":{"id":"turn-error","status":"completed","error":"late failure"}}}),
        ] {
            let (kind, data) = codex_terminal_event(&message, "event-1");
            assert_eq!(kind, "headless.turn.failed");
            assert_eq!(data["event_id"], "event-1");
        }
    }

    #[test]
    fn codex_success_retains_completed_event() {
        let message = json!({
            "params":{"turn":{"id":"turn-completed","status":"completed"}}
        });

        let (kind, data) = codex_terminal_event(&message, "event-1");

        assert_eq!(kind, "headless.turn.completed");
        assert_eq!(data["turn_id"], "turn-completed");
        assert_eq!(data["status"], "completed");
        assert_eq!(data["error"], Value::Null);
    }

    #[test]
    fn claude_error_result_preserves_status_and_error() {
        let message = json!({
            "type":"result",
            "subtype":"error_during_execution",
            "is_error":true,
            "result":"provider failed"
        });

        let (kind, data) = claude_terminal_event(&message, "event-1");

        assert_eq!(kind, "headless.turn.failed");
        assert_eq!(data["event_id"], "event-1");
        assert_eq!(data["status"], "error_during_execution");
        assert_eq!(data["error"]["message"], "provider failed");
    }

    #[test]
    fn claude_error_prefix_is_a_legacy_failure_signal() {
        let (kind, data) = claude_terminal_event(
            &json!({"type":"result","subtype":"error_max_turns","errors":["limit reached"]}),
            "event-1",
        );
        assert_eq!(kind, "headless.turn.failed");
        assert_eq!(data["error"]["message"], "limit reached");

        let (kind, data) = claude_terminal_event(
            &json!({"type":"result","subtype":"success","is_error":false}),
            "event-2",
        );
        assert_eq!(kind, "headless.turn.completed");
        assert_eq!(data["status"], "success");
        assert_eq!(data["error"], Value::Null);
    }
}

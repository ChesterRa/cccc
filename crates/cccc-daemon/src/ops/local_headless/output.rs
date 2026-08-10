use super::{Session, Turn, events};
use cccc_contracts::{ActorRuntime, Event};
use cccc_core::{GroupStore, inbox};
use serde_json::{Map, Value, json};

pub(super) fn handle_message(session: &Session, message: Value) {
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        if let Some(sender) = session
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id))
        {
            let _ = sender.try_send(message);
        }
        return;
    }
    let completed = if session.runtime == ActorRuntime::Codex {
        message.get("method").and_then(Value::as_str) == Some("turn/completed")
    } else {
        message.get("type").and_then(Value::as_str) == Some("result")
    };
    if completed {
        session.set_status("idle", None);
        if let Ok(mut generation) = session.completion.0.lock() {
            *generation += 1;
        }
        session.completion.1.notify_all();
        let event_id = session
            .active_event_id
            .lock()
            .map(|mut value| std::mem::take(&mut *value))
            .unwrap_or_default();
        if session.runtime == ActorRuntime::Codex {
            let (kind, data) = codex_terminal_event(&message, &event_id);
            emit(session, kind, data);
        } else {
            let (kind, data) = claude_terminal_event(&message, &event_id);
            emit(session, kind, data);
        }
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
        if flags.iter().any(|flag| {
            matches!(
                flag.as_str(),
                Some("waitingOnApproval" | "waitingOnUserInput")
            )
        }) {
            let task = session
                .status
                .lock()
                .ok()
                .and_then(|state| state.task_id.clone());
            session.set_status("waiting", task);
        }
    }
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
    let event_id = session
        .active_event_id
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    let key = if kind.ends_with("delta") {
        "delta"
    } else {
        "text"
    };
    emit(
        session,
        kind,
        Map::from_iter([
            ("event_id".into(), json!(event_id)),
            ("stream_id".into(), json!(stream_id)),
            (key.into(), json!(text)),
        ]),
    );
}

pub(super) fn mark_read(session: &Session, turn: &Turn) {
    let _ = inbox::mark_read(
        &session.home,
        &session.group_id,
        &session.actor_id,
        &turn.event_id,
    );
    let Ok(store) = GroupStore::new(session.home.clone()) else {
        return;
    };
    let Ok(path) = store.ledger_path(&session.group_id) else {
        return;
    };
    let mut event = Event::new("chat.read", &session.group_id);
    event.by = session.actor_id.clone();
    event
        .data
        .insert("actor_id".into(), json!(session.actor_id));
    event.data.insert("event_id".into(), json!(turn.event_id));
    event
        .data
        .insert("delivered_ts".into(), json!(turn.event_ts));
    let _ = cccc_core::ledger::append(&path, &event);
}

pub(super) fn emit_turn(session: &Session, turn: &Turn, kind: &str, turn_id: &str) {
    let control_kind = turn.control.then(|| kind.replace("turn", "control"));
    emit(
        session,
        control_kind.as_deref().unwrap_or(kind),
        Map::from_iter([
            ("turn_id".into(), json!(turn_id)),
            ("event_id".into(), json!(turn.event_id)),
        ]),
    );
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

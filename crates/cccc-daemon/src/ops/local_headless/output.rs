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
        emit(
            session,
            "headless.turn.completed",
            Map::from_iter([("event_id".into(), json!(event_id))]),
        );
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

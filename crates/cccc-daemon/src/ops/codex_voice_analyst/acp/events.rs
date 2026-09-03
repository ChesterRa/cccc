use super::tool_results::{publish_mcp_result, remember_tool_call};
use crate::ops::codex_voice_analyst::AnalystEvent;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::broadcast;

#[derive(Debug)]
pub(super) struct ActiveTurn {
    pub(super) turn_id: String,
    pub(super) external: bool,
    pub(super) admitted: bool,
}

#[derive(Debug, Default)]
pub(super) struct ToolCall {
    pub(super) title: String,
    pub(super) raw_input: Value,
}

pub(super) fn handle_notification(
    method: &str,
    message: &Value,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: &mut Option<ActiveTurn>,
    tool_calls: &mut HashMap<String, ToolCall>,
) {
    if message
        .pointer("/params/_meta/isReplay")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return;
    }
    if session_id.is_empty()
        || message
            .get("params")
            .and_then(|params| params.get("sessionId"))
            .and_then(Value::as_str)
            != Some(session_id)
    {
        return;
    }
    if method == "session/update" {
        let update = &message["params"]["update"];
        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "user_message_chunk" => {
                if active.is_none() {
                    let turn_id = format!("acp-tui-{}", uuid::Uuid::new_v4().simple());
                    *active = Some(ActiveTurn {
                        turn_id: turn_id.clone(),
                        external: true,
                        admitted: true,
                    });
                    publish_started(events, generation, session_id, &turn_id, None);
                }
            }
            "agent_message_chunk" => {
                if active.as_ref().is_some_and(|turn| turn.admitted) {
                    publish_agent_delta(update, events, generation, session_id, active.as_ref());
                }
            }
            "tool_call" if active.as_ref().is_some_and(|turn| turn.admitted) => {
                remember_tool_call(update, tool_calls);
            }
            "tool_call_update" => {
                if active.as_ref().is_some_and(|turn| turn.admitted) {
                    remember_tool_call(update, tool_calls);
                    if update.get("status").and_then(Value::as_str) == Some("completed") {
                        publish_mcp_result(
                            update,
                            tool_calls,
                            events,
                            generation,
                            session_id,
                            active.as_ref(),
                        );
                    }
                }
            }
            "turn_completed" => settle_from_update(update, events, generation, session_id, active),
            _ => {}
        }
        return;
    }
    if matches!(
        method,
        "_x.ai/session_notification" | "x.ai/session_notification"
    ) {
        let update = &message["params"]["update"];
        if update["sessionUpdate"] == "turn_completed" {
            settle_from_update(update, events, generation, session_id, active);
        }
    }
    // Grok emits `prompt_complete` as a fire-and-forget duplicate of the
    // persisted `turn_completed` update. It can arrive after the next turn has
    // started and carries no CCCC turn id, so it must never settle `active`.
}

fn publish_agent_delta(
    update: &Value,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: Option<&ActiveTurn>,
) {
    let Some(turn) = active else { return };
    let text = update
        .get("content")
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !text.is_empty() {
        publish(
            events,
            generation,
            json!({
                "method":"item/agentMessage/delta",
                "params":{
                    "threadId":session_id,
                    "turnId":turn.turn_id,
                    "itemId":format!("{}-message", turn.turn_id),
                    "delta":text,
                }
            }),
        );
    }
}

fn settle_from_update(
    update: &Value,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: &mut Option<ActiveTurn>,
) {
    // A CCCC-owned prompt is correlated by its JSON-RPC response in the
    // protocol loop. Provider broadcasts carry only provider prompt ids and
    // could otherwise settle a later CCCC turn. Durable terminal updates are
    // used here only for turns started from the attached native TUI.
    let Some(turn_id) = active
        .as_ref()
        .filter(|turn| turn.external)
        .map(|turn| turn.turn_id.clone())
    else {
        return;
    };
    let reason = update
        .get("stopReason")
        .or_else(|| update.get("stop_reason"))
        .and_then(Value::as_str);
    let (status, error) = status_from_stop_reason(reason);
    settle_turn(
        events,
        generation,
        session_id,
        &turn_id,
        status,
        error.as_deref(),
        active,
    );
}

pub(super) fn status_from_stop_reason(reason: Option<&str>) -> (&'static str, Option<String>) {
    match reason
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "end_turn" | "completed" | "success" => ("completed", None),
        "cancelled" | "canceled" | "interrupted" => ("cancelled", None),
        "" => (
            "failed",
            Some("ACP turn ended without a stop reason".into()),
        ),
        other => ("failed", Some(format!("ACP turn stopped: {other}"))),
    }
}

pub(super) fn settle_turn(
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    expected_turn_id: &str,
    status: &str,
    error: Option<&str>,
    active: &mut Option<ActiveTurn>,
) {
    if active
        .as_ref()
        .is_none_or(|turn| turn.turn_id != expected_turn_id)
    {
        return;
    }
    let turn = active.take().expect("checked active ACP turn");
    publish(
        events,
        generation,
        json!({
            "method":"item/completed",
            "params":{
                "threadId":session_id,
                "turnId":turn.turn_id,
                "item":{
                    "id":format!("{}-message", turn.turn_id),
                    "type":"agentMessage",
                    "text":"",
                }
            }
        }),
    );
    publish(
        events,
        generation,
        json!({
            "method":"turn/completed",
            "params":{
                "threadId":session_id,
                "turn":{
                    "id":turn.turn_id,
                    "status":status,
                    "error":error,
                }
            }
        }),
    );
}

pub(super) fn publish_started(
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    turn_id: &str,
    delegation_id: Option<String>,
) {
    let _ = events.send(AnalystEvent {
        generation: generation.to_owned(),
        message: json!({
            "method":"turn/started",
            "params":{"threadId":session_id,"turn":{"id":turn_id}}
        }),
        requested_delegation_id: delegation_id,
    });
}

pub(super) fn publish(events: &broadcast::Sender<AnalystEvent>, generation: &str, message: Value) {
    let _ = events.send(AnalystEvent {
        generation: generation.to_owned(),
        message,
        requested_delegation_id: None,
    });
}

pub(super) fn publish_approval_required(
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    runtime: &str,
) {
    publish(
        events,
        generation,
        json!({
            "method":"mcpServer/elicitation/request",
            "params":{"threadId":session_id,"managedRuntime":runtime}
        }),
    );
}

#[cfg(test)]
mod tests;

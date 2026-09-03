use super::events::{ActiveTurn, ToolCall, publish};
use crate::ops::codex_voice_analyst::AnalystEvent;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::broadcast;

pub(super) fn remember_tool_call(update: &Value, tool_calls: &mut HashMap<String, ToolCall>) {
    let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
        return;
    };
    let entry = tool_calls.entry(id.to_owned()).or_default();
    if let Some(title) = update.get("title").and_then(Value::as_str) {
        entry.title = title.to_owned();
    }
    if let Some(raw_input) = update.get("rawInput") {
        entry.raw_input = raw_input.clone();
    }
}

pub(super) fn publish_mcp_result(
    update: &Value,
    tool_calls: &mut HashMap<String, ToolCall>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: Option<&ActiveTurn>,
) {
    let Some(turn) = active else { return };
    let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
        return;
    };
    let remembered = tool_calls.remove(id).unwrap_or_default();
    let raw_output = &update["rawOutput"];
    if raw_output.get("type").and_then(Value::as_str) != Some("mcp_tool_result") {
        return;
    }
    let server = raw_output
        .get("server_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tool = raw_output
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if server.is_empty() || tool.is_empty() {
        return;
    }
    let text = raw_output
        .pointer("/output/OkayOutput")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let structured = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let result = if structured.is_null() {
        json!({"content":[{"type":"text","text":text}]})
    } else {
        json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":structured,
        })
    };
    publish(
        events,
        generation,
        json!({
            "method":"item/completed",
            "params":{
                "threadId":session_id,
                "turnId":turn.turn_id,
                "item":{
                    "id":id,
                    "type":"mcpToolCall",
                    "status":"completed",
                    "server":server,
                    "tool":tool,
                    "title":remembered.title,
                    "arguments":{"tool_arguments":remembered.raw_input},
                    "result":result,
                }
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cccc_mcp_text_is_restored_to_structured_content() {
        let (events, mut receiver) = broadcast::channel(8);
        let active = ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: true,
        };
        let mut calls = HashMap::from([(
            "call-1".into(),
            ToolCall {
                title: "Send".into(),
                raw_input: json!({"group_id":"g1"}),
            },
        )]);
        publish_mcp_result(
            &json!({
                "toolCallId":"call-1",
                "status":"completed",
                "rawOutput":{
                    "type":"mcp_tool_result",
                    "server_name":"cccc",
                    "tool_name":"cccc_tracked_send",
                    "output":{"OkayOutput":"{\"task_id\":\"task-1\"}"}
                }
            }),
            &mut calls,
            &events,
            "generation",
            "session-1",
            Some(&active),
        );
        let event = receiver.try_recv().expect("normalized event");
        assert_eq!(event.message["params"]["item"]["server"], "cccc");
        assert_eq!(
            event.message["params"]["item"]["result"]["structuredContent"]["task_id"],
            "task-1"
        );
    }

    #[test]
    fn completed_non_mcp_tools_do_not_accumulate_session_state() {
        let (events, _) = broadcast::channel(8);
        let active = ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: true,
        };
        let mut calls = HashMap::from([(
            "call-1".into(),
            ToolCall {
                title: "Shell".into(),
                raw_input: json!({"command":"true"}),
            },
        )]);
        publish_mcp_result(
            &json!({
                "toolCallId":"call-1",
                "status":"completed",
                "rawOutput":{"type":"Bash","exit_code":0}
            }),
            &mut calls,
            &events,
            "generation",
            "session-1",
            Some(&active),
        );
        assert!(calls.is_empty());
    }
}

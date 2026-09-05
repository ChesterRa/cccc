use super::super::{AnalystEvent, MANAGED_AGENT_DELEGATION_ATTACHED_METHOD};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io;
use tokio::sync::broadcast;

const INTERRUPTION_MARKER: &str = "[Request interrupted by user]";
const TOOL_INTERRUPTION_MARKER: &str = "[Request interrupted by user for tool use]";

pub(super) struct PendingPrompt<'a> {
    pub(super) delegation_id: &'a str,
    pub(super) text: &'a str,
    pub(super) turn_id: &'a str,
}

pub(super) struct PendingNativeInput<'a> {
    pub(super) delegation_id: &'a str,
    pub(super) text: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum IngestOutcome {
    None,
    Controlled(String),
    Native(String),
}

#[derive(Debug)]
struct ToolCall {
    name: String,
    input: Value,
}

#[derive(Debug)]
struct ActiveTurn {
    turn_id: String,
    prompt_ids: HashSet<String>,
    text: String,
    error: Option<String>,
    tools: HashMap<String, ToolCall>,
}

pub(super) struct TranscriptState {
    generation: String,
    session_id: String,
    events: broadcast::Sender<AnalystEvent>,
    active: Option<ActiveTurn>,
}

impl TranscriptState {
    pub(super) fn new(
        generation: String,
        session_id: String,
        events: broadcast::Sender<AnalystEvent>,
    ) -> Self {
        Self {
            generation,
            session_id,
            events,
            active: None,
        }
    }

    pub(super) fn active_turn_id(&self) -> Option<&str> {
        self.active.as_ref().map(|turn| turn.turn_id.as_str())
    }

    #[cfg(test)]
    pub(super) fn ingest(
        &mut self,
        record: &Value,
        controlled: Option<PendingPrompt<'_>>,
    ) -> io::Result<Option<String>> {
        match self.ingest_with_native(record, controlled, None)? {
            IngestOutcome::Controlled(turn_id) => Ok(Some(turn_id)),
            IngestOutcome::None => Ok(None),
            IngestOutcome::Native(_) => unreachable!("test ingest has no native registration"),
        }
    }

    /// Ingest one complete Claude transcript record. The outcome identifies the exact CCCC input
    /// whose authoritative transcript echo established ownership.
    pub(super) fn ingest_with_native(
        &mut self,
        record: &Value,
        controlled: Option<PendingPrompt<'_>>,
        native: Option<PendingNativeInput<'_>>,
    ) -> io::Result<IngestOutcome> {
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || record.get("isMeta").and_then(Value::as_bool) == Some(true)
        {
            return Ok(IngestOutcome::None);
        }
        if let Some(observed) = record
            .get("sessionId")
            .or_else(|| record.get("session_id"))
            .and_then(Value::as_str)
            && observed != self.session_id
        {
            return invalid("Claude transcript record belongs to a different session");
        }
        if super::transcript_ack::is_resume_ack(record) {
            return Ok(IngestOutcome::None);
        }
        match record.get("type").and_then(Value::as_str) {
            Some("user") => self.ingest_user(record, controlled, native),
            Some("assistant") => {
                self.ingest_assistant(record)?;
                Ok(IngestOutcome::None)
            }
            Some("system")
                if record.get("subtype").and_then(Value::as_str) == Some("turn_duration") =>
            {
                self.settle("completed", None);
                Ok(IngestOutcome::None)
            }
            _ => Ok(IngestOutcome::None),
        }
    }

    fn ingest_user(
        &mut self,
        record: &Value,
        controlled: Option<PendingPrompt<'_>>,
        native: Option<PendingNativeInput<'_>>,
    ) -> io::Result<IngestOutcome> {
        let content = record.pointer("/message/content").unwrap_or(&Value::Null);
        if contains_tool_result(content) {
            self.ingest_tool_results(content);
            return Ok(IngestOutcome::None);
        }
        let text = text_content(content);
        if matches!(text.trim(), INTERRUPTION_MARKER | TOOL_INTERRUPTION_MARKER) {
            let prompt_id = required_prompt_id(record)?;
            if self
                .active
                .as_ref()
                .is_some_and(|turn| turn.prompt_ids.contains(prompt_id))
            {
                self.settle("cancelled", None);
                return Ok(IngestOutcome::None);
            }
            return invalid("Claude interruption marker did not match the active turn");
        }
        if text.trim().is_empty() {
            return Ok(IngestOutcome::None);
        }
        if let Some(active_turn_id) = self.active_turn_id().map(str::to_owned) {
            if let Some(prompt_id) = record
                .get("promptId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                self.active
                    .as_mut()
                    .expect("active turn")
                    .prompt_ids
                    .insert(prompt_id.to_owned());
            }
            if let Some(native) = native.filter(|native| native.text == text) {
                self.publish(
                    json!({
                        "method":MANAGED_AGENT_DELEGATION_ATTACHED_METHOD,
                        "params":{"threadId":self.session_id,"turnId":active_turn_id}
                    }),
                    Some(native.delegation_id.to_owned()),
                );
                return Ok(IngestOutcome::Native(native.delegation_id.to_owned()));
            }
            // Claude's native terminal accepts follow-up input while a turn is active and owns
            // whether that input steers or queues. An unregistered human correction remains part
            // of the same visible turn and must not invalidate the managed session.
            return Ok(IngestOutcome::None);
        }
        let prompt_id = required_prompt_id(record)?.to_owned();
        let (turn_id, requested_delegation_id, outcome) = match controlled {
            Some(controlled) if controlled.text == text => (
                controlled.turn_id.to_owned(),
                Some(controlled.delegation_id.to_owned()),
                IngestOutcome::Controlled(controlled.turn_id.to_owned()),
            ),
            Some(_) => {
                return invalid(
                    "terminal input raced a pending CCCC Claude prompt; turn ownership is ambiguous",
                );
            }
            None => match native.filter(|native| native.text == text) {
                Some(native) => (
                    format!("claude-{prompt_id}"),
                    Some(native.delegation_id.to_owned()),
                    IngestOutcome::Native(native.delegation_id.to_owned()),
                ),
                None => (format!("claude-{prompt_id}"), None, IngestOutcome::None),
            },
        };
        self.active = Some(ActiveTurn {
            turn_id: turn_id.clone(),
            prompt_ids: HashSet::from([prompt_id]),
            text: String::new(),
            error: None,
            tools: HashMap::new(),
        });
        self.publish(
            json!({
                "method":"turn/started",
                "params":{"threadId":self.session_id,"turn":{"id":turn_id}}
            }),
            requested_delegation_id.clone(),
        );
        Ok(outcome)
    }

    fn ingest_assistant(&mut self, record: &Value) -> io::Result<()> {
        let mut messages = Vec::new();
        let Some(active) = self.active.as_mut() else {
            return invalid("Claude emitted assistant output without an active transcript turn");
        };
        let content = record.pointer("/message/content").unwrap_or(&Value::Null);
        for block in content.as_array().into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !text.is_empty() {
                        active.text.push_str(text);
                        let message = json!({
                            "method":"item/agentMessage/delta",
                            "params":{
                                "threadId":self.session_id,
                                "turnId":active.turn_id,
                                "itemId":format!("{}-message", active.turn_id),
                                "delta":text,
                            }
                        });
                        messages.push(message);
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !id.is_empty() && !name.is_empty() {
                        active.tools.insert(
                            id.to_owned(),
                            ToolCall {
                                name: name.to_owned(),
                                input: block.get("input").cloned().unwrap_or(Value::Null),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        if record.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
            || record.get("error").is_some_and(|value| !value.is_null())
        {
            let code = record
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("provider_error");
            let detail = text_content(content);
            active.error = Some(if detail.trim().is_empty() {
                format!("Claude turn failed: {code}")
            } else {
                format!("Claude turn failed ({code}): {}", detail.trim())
            });
        }
        for message in messages {
            self.publish(message, None);
        }
        Ok(())
    }

    fn ingest_tool_results(&mut self, content: &Value) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let mut messages = Vec::new();
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(call) = active.tools.remove(id) else {
                continue;
            };
            let Some((server, tool)) = managed_tool_name(&call.name) else {
                continue;
            };
            if server != "cccc" {
                continue;
            }
            let text = text_content(block.get("content").unwrap_or(&Value::Null));
            let structured = serde_json::from_str::<Value>(&text).ok();
            let mut result = json!({"content":[{"type":"text","text":text}]});
            if let Some(structured) = structured {
                result["structuredContent"] = structured;
            }
            let status = if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                "failed"
            } else {
                "completed"
            };
            messages.push(json!({
                "method":"item/completed",
                "params":{
                    "threadId":self.session_id,
                    "turnId":active.turn_id,
                    "item":{
                        "id":id,
                        "type":"mcpToolCall",
                        "status":status,
                        "server":server,
                        "tool":tool,
                        "title":tool,
                        "arguments":{"tool_arguments":call.input},
                        "result":result,
                    }
                }
            }));
        }
        for message in messages {
            self.publish(message, None);
        }
    }

    fn settle(&mut self, requested_status: &str, requested_error: Option<&str>) {
        let Some(active) = self.active.take() else {
            return;
        };
        let error = requested_error.map(str::to_owned).or(active.error);
        let status = if error.is_some() {
            "failed"
        } else {
            requested_status
        };
        self.publish(
            json!({
                "method":"item/completed",
                "params":{
                    "threadId":self.session_id,
                    "turnId":active.turn_id,
                    "item":{
                        "id":format!("{}-message", active.turn_id),
                        "type":"agentMessage",
                        "text":active.text,
                    }
                }
            }),
            None,
        );
        self.publish(
            json!({
                "method":"turn/completed",
                "params":{
                    "threadId":self.session_id,
                    "turn":{
                        "id":active.turn_id,
                        "status":status,
                        "error":error,
                    }
                }
            }),
            None,
        );
    }

    fn publish(&self, message: Value, requested_delegation_id: Option<String>) {
        let _ = self.events.send(AnalystEvent {
            generation: self.generation.clone(),
            message,
            requested_delegation_id,
        });
    }
}

fn required_prompt_id(record: &Value) -> io::Result<&str> {
    record
        .get("promptId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude user record omitted promptId",
            )
        })
}

fn contains_tool_result(content: &Value) -> bool {
    content.as_array().is_some_and(|blocks| {
        blocks
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
    })
}

fn text_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn managed_tool_name(value: &str) -> Option<(&str, &str)> {
    value.strip_prefix("mcp__")?.split_once("__")
}

fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn harness() -> (TranscriptState, broadcast::Receiver<AnalystEvent>) {
        let (events, receiver) = broadcast::channel(32);
        (
            TranscriptState::new("generation-1".into(), "session-1".into(), events),
            receiver,
        )
    }

    #[test]
    fn controlled_prompt_is_correlated_and_completed_once() {
        let (mut state, mut events) = harness();
        let turn_id = state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":"inspect this"}
                }),
                Some(PendingPrompt {
                    delegation_id: "delegation-1",
                    text: "inspect this",
                    turn_id: "claude-controlled-1",
                }),
            )
            .expect("user record")
            .expect("matched prompt");
        assert_eq!(turn_id, "claude-controlled-1");
        let started = events.try_recv().expect("started");
        assert_eq!(
            started.requested_delegation_id.as_deref(),
            Some("delegation-1")
        );

        state
            .ingest(
                &json!({
                    "type":"assistant","sessionId":"session-1",
                    "message":{"content":[{"type":"text","text":"answer"}]}
                }),
                None,
            )
            .expect("assistant");
        state
            .ingest(
                &json!({"type":"system","sessionId":"session-1","subtype":"turn_duration"}),
                None,
            )
            .expect("terminal");
        assert_eq!(
            events.try_recv().expect("delta").message["method"],
            "item/agentMessage/delta"
        );
        let completed_item = events.try_recv().expect("completed item");
        assert_eq!(completed_item.message["params"]["item"]["text"], "answer");
        let completed_turn = events.try_recv().expect("completed turn");
        assert_eq!(
            completed_turn.message["params"]["turn"]["status"],
            "completed"
        );
        assert!(state.active_turn_id().is_none());
    }

    #[test]
    fn terminal_race_is_rejected_instead_of_misattributed() {
        let (mut state, _) = harness();
        let error = state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-terminal",
                    "message":{"content":"terminal input"}
                }),
                Some(PendingPrompt {
                    delegation_id: "delegation-1",
                    text: "controlled input",
                    turn_id: "claude-controlled-1",
                }),
            )
            .expect_err("ambiguous ownership");
        assert!(error.to_string().contains("ownership is ambiguous"));
    }

    #[test]
    fn native_follow_up_is_correlated_without_splitting_or_invalidating_the_turn() {
        let (mut state, mut events) = harness();
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":"initial investigation"}
                }),
                Some(PendingPrompt {
                    delegation_id: "delegation-1",
                    text: "initial investigation",
                    turn_id: "claude-controlled-1",
                }),
            )
            .expect("controlled start");
        let _ = events.try_recv().expect("started");

        assert_eq!(
            state
                .ingest_with_native(
                    &json!({
                        "type":"user","sessionId":"session-1","promptId":"prompt-2",
                        "message":{"content":"include the new constraint"}
                    }),
                    None,
                    Some(PendingNativeInput {
                        delegation_id: "delegation-2",
                        text: "include the new constraint",
                    }),
                )
                .expect("native follow-up"),
            IngestOutcome::Native("delegation-2".into())
        );
        let associated = events.try_recv().expect("associated");
        assert_eq!(
            associated.message["method"],
            MANAGED_AGENT_DELEGATION_ATTACHED_METHOD
        );
        assert_eq!(
            associated.requested_delegation_id.as_deref(),
            Some("delegation-2")
        );
        assert_eq!(
            associated.message["params"]["turnId"],
            "claude-controlled-1"
        );

        assert_eq!(
            state
                .ingest_with_native(
                    &json!({
                        "type":"user","sessionId":"session-1","promptId":"prompt-manual",
                        "message":{"content":"a human terminal correction"}
                    }),
                    None,
                    None,
                )
                .expect("manual follow-up"),
            IngestOutcome::None
        );
        assert_eq!(state.active_turn_id(), Some("claude-controlled-1"));
    }

    #[test]
    fn resume_meta_prompt_is_ignored_before_the_real_user_prompt() {
        let (mut state, mut events) = harness();
        assert_eq!(
            state
                .ingest(
                    &json!({
                        "type":"user","sessionId":"session-1","promptId":"prompt-1",
                        "isMeta":true,
                        "message":{"content":"Continue from where you left off."}
                    }),
                    Some(PendingPrompt {
                        delegation_id: "delegation-1",
                        text: "inspect this",
                        turn_id: "claude-controlled-1",
                    }),
                )
                .expect("resume metadata"),
            None
        );
        assert!(events.try_recv().is_err());

        assert_eq!(
            state
                .ingest(
                    &json!({
                        "type":"user","sessionId":"session-1","promptId":"prompt-1",
                        "message":{"content":"inspect this"}
                    }),
                    Some(PendingPrompt {
                        delegation_id: "delegation-1",
                        text: "inspect this",
                        turn_id: "claude-controlled-1",
                    }),
                )
                .expect("real user prompt")
                .as_deref(),
            Some("claude-controlled-1")
        );
        let started = events.try_recv().expect("one started event");
        assert_eq!(started.message["method"], "turn/started");
        assert_eq!(
            started.requested_delegation_id.as_deref(),
            Some("delegation-1")
        );
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn interruption_and_provider_error_have_terminal_statuses() {
        let (mut state, mut events) = harness();
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":"slow work"}
                }),
                None,
            )
            .expect("external start");
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":[{"type":"text","text":INTERRUPTION_MARKER}]}
                }),
                None,
            )
            .expect("interrupt");
        let statuses = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| event.message["method"] == "turn/completed")
            .map(|event| event.message["params"]["turn"]["status"].clone())
            .collect::<Vec<_>>();
        assert_eq!(statuses, [json!("cancelled")]);

        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-2",
                    "message":{"content":"next"}
                }),
                None,
            )
            .expect("second start");
        state
            .ingest(
                &json!({
                    "type":"assistant","sessionId":"session-1",
                    "error":"authentication_failed","isApiErrorMessage":true,
                    "message":{"content":[{"type":"text","text":"Not logged in"}]}
                }),
                None,
            )
            .expect("provider error");
        state
            .ingest(
                &json!({"type":"system","sessionId":"session-1","subtype":"turn_duration"}),
                None,
            )
            .expect("error terminal");
        let failed = std::iter::from_fn(|| events.try_recv().ok())
            .find(|event| event.message["method"] == "turn/completed")
            .expect("failed turn");
        assert_eq!(failed.message["params"]["turn"]["status"], "failed");
        assert!(
            failed.message["params"]["turn"]["error"]
                .as_str()
                .is_some_and(|value| value.contains("authentication_failed"))
        );
    }

    #[test]
    fn cccc_tool_result_preserves_structured_tracking_payload() {
        let (mut state, mut events) = harness();
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1","promptId":"prompt-1",
                    "message":{"content":"delegate"}
                }),
                None,
            )
            .expect("start");
        state
            .ingest(
                &json!({
                    "type":"assistant","sessionId":"session-1",
                    "message":{"content":[{
                        "type":"tool_use","id":"tool-1",
                        "name":"mcp__cccc__cccc_tracked_send",
                        "input":{"group_id":"g1"}
                    }]}
                }),
                None,
            )
            .expect("tool use");
        state
            .ingest(
                &json!({
                    "type":"user","sessionId":"session-1",
                    "message":{"content":[{
                        "type":"tool_result","tool_use_id":"tool-1",
                        "content":[{"type":"text","text":"{\"task_id\":\"task-1\"}"}]
                    }]}
                }),
                None,
            )
            .expect("tool result");
        let tracked = std::iter::from_fn(|| events.try_recv().ok())
            .find(|event| event.message["params"]["item"]["type"] == "mcpToolCall")
            .expect("normalized tool result");
        assert_eq!(tracked.message["params"]["item"]["server"], "cccc");
        assert_eq!(
            tracked.message["params"]["item"]["tool"],
            "cccc_tracked_send"
        );
        assert_eq!(
            tracked.message["params"]["item"]["result"]["structuredContent"]["task_id"],
            "task-1"
        );
    }
}

#[cfg(test)]
#[path = "transcript_resume_ack_tests.rs"]
mod resume_ack_tests;

#[cfg(test)]
#[path = "transcript_interrupt_tests.rs"]
mod interrupt_tests;

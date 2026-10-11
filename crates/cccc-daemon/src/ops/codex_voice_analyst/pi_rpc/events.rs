use super::{AnalystEvent, Value, json};
use std::collections::BTreeMap;
use std::io;
use tokio::sync::{broadcast, oneshot};

pub(super) struct Turn {
    pub id: String,
    pub delegation_id: String,
    pub text: String,
    pub admitted: bool,
    pub provider_started: bool,
    pub cancelling: bool,
    pub admission: Option<oneshot::Sender<io::Result<String>>>,
    final_text: String,
    provider_failed: bool,
    message: u64,
    tools: BTreeMap<String, Value>,
}

impl Turn {
    pub(super) fn new(
        delegation_id: String,
        text: String,
        admission: oneshot::Sender<io::Result<String>>,
    ) -> Self {
        Self {
            id: format!("pi-{}", uuid::Uuid::new_v4().simple()),
            delegation_id,
            text,
            admitted: false,
            provider_started: false,
            cancelling: false,
            admission: Some(admission),
            final_text: String::new(),
            provider_failed: false,
            message: 0,
            tools: BTreeMap::new(),
        }
    }

    pub(super) fn admit(
        &mut self,
        sender: &broadcast::Sender<AnalystEvent>,
        generation: &str,
        session: &str,
    ) {
        if self.admitted {
            return;
        }
        self.admitted = true;
        publish(
            sender,
            generation,
            Some(&self.delegation_id),
            json!({"method":"turn/started","params":{"threadId":session,"turn":{"id":self.id,"status":"inProgress"}}}),
        );
        if let Some(response) = self.admission.take() {
            let _ = response.send(Ok(self.id.clone()));
        }
    }

    /// The RPC port has a single fixed owner. Only the exact live controlled
    /// user record (or a blocking dialog handling it) admits that owner's turn.
    pub(super) fn observe(
        &mut self,
        record: &Value,
        sender: &broadcast::Sender<AnalystEvent>,
        generation: &str,
        session: &str,
    ) -> io::Result<bool> {
        let typ = record["type"].as_str().unwrap_or_default();
        if matches!(typ, "message_start" | "message_end") && record["message"]["role"] == "user" {
            let text = message_text(&record["message"]);
            if text != self.text {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Pi emitted a different user prompt; controlled ownership was not inferred",
                ));
            }
            self.provider_started = true;
            self.admit(sender, generation, session);
        }
        if typ == "agent_settled" {
            if !self.admitted && self.cancelling {
                return Ok(true);
            }
            if !self.admitted {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Pi settled without admitting the controlled prompt",
                ));
            }
            let status = if self.cancelling || record["aborted"] == true {
                "interrupted"
            } else if self.provider_failed {
                "failed"
            } else {
                "completed"
            };
            self.finish(sender, generation, session, status);
            return Ok(true);
        }
        if !self.admitted {
            return Ok(false);
        }
        match typ {
            "message_start" if record["message"]["role"] == "assistant" => {
                self.message += 1;
            }
            "message_update" => {
                let update = &record["assistantMessageEvent"];
                if let Some("text_delta") = update["type"].as_str() {
                    let delta = update["delta"].as_str().unwrap_or_default();
                    publish(
                        sender,
                        generation,
                        None,
                        json!({"method":"item/agentMessage/delta","params":{"threadId":session,"turnId":self.id,"itemId":format!("pi-message-{}",self.message),"delta":delta}}),
                    );
                }
            }
            "message_end" if record["message"]["role"] == "assistant" => {
                // Authoritative snapshots replace the streamed reconstruction.
                self.final_text = message_text(&record["message"]);
                self.provider_failed = matches!(
                    record["message"]["stopReason"].as_str(),
                    Some("error" | "aborted")
                );
                publish(
                    sender,
                    generation,
                    None,
                    json!({"method":"item/completed","params":{"threadId":session,"turnId":self.id,"item":{"id":format!("pi-message-{}",self.message),"type":"agentMessage","text":self.final_text}}}),
                );
            }
            "auto_retry_end" if record["success"] == false => self.provider_failed = true,
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                let title = record["toolName"].as_str().unwrap_or_default();
                publish(
                    sender,
                    generation,
                    None,
                    json!({"method":"cccc/toolActivity","params":{"threadId":session,"turnId":self.id,"title":title.chars().take(400).collect::<String>()}}),
                );
                let id = record["toolCallId"].as_str().unwrap_or_default();
                if typ == "tool_execution_start" && title.starts_with("mcp__cccc__") {
                    self.tools.insert(id.into(), record["args"].clone());
                }
                if typ == "tool_execution_end" {
                    let args = self.tools.remove(id).unwrap_or(Value::Null);
                    if let Some(tool) = title.strip_prefix("mcp__cccc__") {
                        let mut result = record["result"].clone();
                        if result.get("structuredContent").is_none()
                            && let Some(text) = result["content"].as_array().and_then(|content| {
                                content.iter().find_map(|part| part["text"].as_str())
                            })
                            && let Ok(structured) = serde_json::from_str::<Value>(text)
                        {
                            result["structuredContent"] = structured;
                        }
                        publish(
                            sender,
                            generation,
                            None,
                            json!({"method":"item/completed","params":{"threadId":session,"turnId":self.id,"item":{"id":id,"type":"mcpToolCall","status":if record["isError"]==true{"failed"}else{"completed"},"server":"cccc","tool":tool,"arguments":{"tool_arguments":args},"result":result}}}),
                        );
                    }
                }
            }
            // agent_end/turn_end close lower-level operations. Retry, compaction
            // and queued work can follow; they never settle a CCCC turn.
            _ => {}
        }
        Ok(false)
    }

    pub(super) fn finish(
        &self,
        sender: &broadcast::Sender<AnalystEvent>,
        generation: &str,
        session: &str,
        status: &str,
    ) {
        let error=(status=="failed").then(||json!({"message":"Pi provider turn failed; check the selected provider configuration"}));
        publish(
            sender,
            generation,
            None,
            json!({"method":"turn/completed","params":{"threadId":session,"turn":{"id":self.id,"status":status,"error":error}}}),
        );
    }
}

fn message_text(message: &Value) -> String {
    message["content"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
}

pub(super) fn publish(
    sender: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    delegation: Option<&str>,
    message: Value,
) {
    let _ = sender.send(AnalystEvent {
        generation: generation.into(),
        message,
        requested_delegation_id: delegation.map(str::to_owned),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_settled_completes_and_authoritative_message_replaces_deltas() {
        let (sender, mut receiver) = broadcast::channel(32);
        let (admission, _) = oneshot::channel();
        let mut turn = Turn::new("d".into(), "exact".into(), admission);
        assert!(
            !turn
                .observe(
                    &json!({"type":"message_start","message":{"role":"user","content":"exact"}}),
                    &sender,
                    "g",
                    "s"
                )
                .expect("admit")
        );
        for record in [
            json!({"type":"message_start","message":{"role":"assistant"}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"partial","contentIndex":0}}),
            json!({"type":"message_update","assistantMessageEvent":{"type":"text_end","content":"replaced","contentIndex":0}}),
            json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"final"}],"stopReason":"stop"}}),
            json!({"type":"agent_end","willRetry":true}),
        ] {
            assert!(!turn.observe(&record, &sender, "g", "s").expect("event"));
        }
        assert_eq!(turn.final_text, "final");
        assert!(
            turn.observe(
                &json!({"type":"agent_settled","aborted":false}),
                &sender,
                "g",
                "s"
            )
            .expect("settled")
        );
        let mut terminals = 0;
        while let Ok(event) = receiver.try_recv() {
            terminals += usize::from(event.message["method"] == "turn/completed");
        }
        assert_eq!(terminals, 1);
    }
}

use super::*;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    pub(super) active: Option<Turn>,
    pub(super) controlled: Option<(String, String)>,
    pub(super) native: VecDeque<native_input::PendingNativeInput>,
    pending: HashMap<String, Value>,
    tools: HashMap<String, Tool>,
    last_sequence: Option<u64>,
}

pub(super) struct Turn {
    pub(super) id: String,
    assistant: Option<String>,
    finish: Option<String>,
}

#[derive(Default)]
struct Tool {
    name: String,
    input: Value,
}

fn string<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("OpenCode V2 event is missing {key}"),
            )
        })
}

impl State {
    pub(super) fn observe(&mut self, inner: &Inner, event: &Value) -> io::Result<()> {
        let data = &event["data"];
        if data["sessionID"] != inner.session_id {
            return Ok(());
        }
        if let Some(sequence) = event["durable"]["seq"].as_u64() {
            if self.last_sequence.is_some_and(|last| sequence <= last) {
                return Ok(());
            }
            self.last_sequence = Some(sequence);
        }
        match event["type"].as_str().unwrap_or_default() {
            "session.inbox.enqueued" if data["item"]["type"] == "user" => {
                if self.pending.len() >= 256 {
                    return Err(io::Error::other(
                        "OpenCode V2 queued-input observation limit exceeded",
                    ));
                }
                self.pending
                    .insert(string(data, "inboxID")?.into(), data["item"].clone());
            }
            "session.inbox.cancelled" => {
                let id = string(data, "inboxID")?;
                self.pending.remove(id);
                if self
                    .controlled
                    .as_ref()
                    .is_some_and(|(expected, _)| expected == id)
                {
                    let (_, delegation) = self.controlled.take().expect("matching input");
                    // Durable admission already returned this turn's receipt.
                    // Correlate its cancellation even if it raced with the HTTP
                    // response. Do not announce an overlapping native turn.
                    if self.active.is_none() {
                        inner.publish(json!({"method":"turn/started","params":{"threadId":inner.session_id,"turn":{"id":id}}}),Some(delegation));
                    }
                    inner.publish(json!({"method":"turn/completed","params":{"threadId":inner.session_id,"turn":{"id":id,"status":"cancelled"}}}),None);
                }
            }
            "session.inbox.delivery.changed" => {
                if let Some(item) = self.pending.get_mut(string(data, "inboxID")?) {
                    item["delivery"] = json!(string(data, "delivery")?);
                }
            }
            "session.inbox.delivered" => {
                let id = string(data, "inboxID")?;
                // Other durable work (e.g. compaction) has no user intent.
                let Some(item) = self.pending.remove(id) else {
                    return Ok(());
                };
                let delegation = if self
                    .controlled
                    .as_ref()
                    .is_some_and(|(expected, _)| expected == id)
                {
                    self.controlled.take().map(|(_, delegation)| delegation)
                } else {
                    native_input::observe(&mut self.native, string(&item["payload"], "text")?)
                };
                if item["delivery"] == "queue" || self.active.is_none() {
                    if self.active.is_some() {
                        self.finish(inner, "completed", None);
                    }
                    self.active = Some(Turn {
                        id: id.into(),
                        assistant: None,
                        finish: None,
                    });
                    self.started(inner, delegation);
                } else if delegation.is_some() {
                    // Native steering contributes to the existing execution.
                    self.started(inner, delegation);
                }
            }
            "session.step.started" => {
                let turn = self.active.as_mut().ok_or_else(|| {
                    io::Error::other("OpenCode V2 began a step without observed input")
                })?;
                turn.assistant = Some(string(data, "assistantMessageID")?.into());
                turn.finish = None;
            }
            "session.text.delta"
            | "session.text.ended"
            | "session.tool.input.started"
            | "session.tool.called"
            | "session.tool.success"
            | "session.tool.failed"
            | "session.step.ended"
            | "session.step.failed" => {
                let Some(turn) = self
                    .active
                    .as_mut()
                    .filter(|t| t.assistant.as_deref() == data["assistantMessageID"].as_str())
                else {
                    return Ok(());
                };
                match event["type"].as_str().unwrap_or_default() {
                    "session.text.delta" => inner.publish(json!({
                        "method":"item/agentMessage/delta",
                        "params":{
                            "threadId":inner.session_id,"turnId":turn.id,
                            "itemId":format!("{}:{}",data["assistantMessageID"].as_str().unwrap_or_default(),data["ordinal"].as_u64().unwrap_or_default()),
                            "delta":data["delta"].as_str().ok_or_else(||io::Error::other("OpenCode text delta is missing"))?,
                        }
                    }),None),
                    "session.text.ended" => inner.publish(json!({
                        "method":"item/completed",
                        "params":{
                            "threadId":inner.session_id,"turnId":turn.id,
                            "item":{
                                "id":format!("{}:{}",data["assistantMessageID"].as_str().unwrap_or_default(),data["ordinal"].as_u64().unwrap_or_default()),
                                "type":"agentMessage",
                                "text":data["text"].as_str().ok_or_else(||io::Error::other("OpenCode completed text is missing"))?,
                            }
                        }
                    }),None),
                    "session.tool.input.started" => {
                        if self.tools.len()>=4096 {return Err(io::Error::other("OpenCode V2 tool observation limit exceeded"));}
                        self.tools.insert(string(data,"id")?.into(),Tool{name:string(data,"name")?.into(),input:Value::Null});
                    }
                    "session.tool.called" => {
                        if let Some(tool)=self.tools.get_mut(string(data,"id")?) {
                            tool.input=data["input"].clone();
                            inner.publish(json!({"method":"cccc/toolActivity","params":{"threadId":inner.session_id,"turnId":turn.id,"title":tool.name,"status":"in_progress"}}),None);
                        }
                    }
                    "session.tool.success" | "session.tool.failed" => {
                        let Some(tool)=self.tools.remove(string(data,"id")?) else{return Ok(());};
                        if event["type"]=="session.tool.success" {
                            // V2's public MCP namespace is `${server}_${tool}`.
                            // Only our injected server's direct calls are receipts.
                            if let Some(name)=tool.name.strip_prefix("cccc_") {
                                let text=data["content"].as_array().into_iter().flatten().filter(|v|v["type"]=="text").filter_map(|v|v["text"].as_str()).collect::<Vec<_>>().join("\n");
                                let structured=serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
                                inner.publish(json!({"method":"item/completed","params":{"threadId":inner.session_id,"turnId":turn.id,"item":{"id":data["id"],"type":"mcpToolCall","status":"completed","server":"cccc","tool":name,"arguments":{"tool_arguments":tool.input},"result":{"content":data["content"],"structuredContent":structured}}}}),None);
                            }
                        }
                    }
                    "session.step.ended" => turn.finish=Some(string(data,"finish")?.into()),
                    "session.step.failed" => turn.finish=Some("failed".into()),
                    _=>{}
                }
            }
            "session.execution.succeeded" => self.finish(inner, "completed", None),
            "session.execution.failed" => {
                self.finish(inner, "failed", Some("OpenCode V2 execution failed"))
            }
            "session.execution.interrupted" => self.finish(inner, "cancelled", None),
            "session.deleted" => return Err(io::Error::other("OpenCode V2 session was deleted")),
            _ => {}
        }
        Ok(())
    }

    fn started(&self, inner: &Inner, delegation: Option<String>) {
        if let Some(turn) = &self.active {
            inner.publish(json!({"method":"turn/started","params":{"threadId":inner.session_id,"turn":{"id":turn.id}}}),delegation);
        }
    }

    fn finish(&mut self, inner: &Inner, status: &str, error: Option<&str>) {
        let Some(turn) = self.active.take() else {
            return;
        };
        let (status, error) = if status == "completed" && turn.finish.as_deref() != Some("stop") {
            (
                "failed",
                Some("OpenCode V2 execution ended without a successful final model step"),
            )
        } else {
            (status, error)
        };
        inner.publish(json!({"method":"turn/completed","params":{"threadId":inner.session_id,"turn":{"id":turn.id,"status":status,"error":error}}}),None);
        self.tools.clear();
    }
}

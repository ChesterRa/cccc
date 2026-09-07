//! Project output and turn boundaries from one ordered backend subscription.
//! ACP is still the control port; its separate subscriber/RPC responses cannot
//! order a queued native answer relative to the preceding controlled prompt.
use super::super::acp::AcpLifecycleControl;
use super::lifecycle::{ObservedUserMessage, observed_user_text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;

const MAX_PENDING_USERS: usize = 256;
const MAX_TRACKED_PARTS: usize = 4096;

#[derive(Default)]
pub(in super::super) struct SessionStream {
    current_user: Option<ObservedUserMessage>,
    observed_user_part: Option<String>,
    pending_users: VecDeque<(String, String)>,
    synchronized_model: Option<String>,
    assistant_messages: HashSet<String>,
    text_parts: HashMap<String, String>,
    error: Option<Value>,
}

impl SessionStream {
    pub(in super::super) async fn observe(
        &mut self,
        payload: &Value,
        session_id: &str,
        control: &AcpLifecycleControl,
    ) -> io::Result<()> {
        if let Some(text) = observed_user_text(payload, session_id, &mut self.current_user) {
            let part = &payload["properties"]["part"];
            let part_id = required_string(part, "id")?;
            if self.observed_user_part.as_deref() != Some(part_id) {
                self.observed_user_part = Some(part_id.to_owned());
                let native = control.observe_user_text(session_id, &text, false).await?;
                let user = self.current_user.as_ref().expect("observed user text");
                if native {
                    if self.pending_users.len() >= MAX_PENDING_USERS {
                        return Err(io::Error::other(
                            "OpenCode pending user-message limit exceeded",
                        ));
                    }
                    self.pending_users.push_back((user.id.clone(), text));
                    if let Some(model) = &user.model
                        && self.synchronized_model.as_ref() != Some(model)
                    {
                        control
                            .set_config_option(session_id, "model", model)
                            .await?;
                        self.synchronized_model = Some(model.clone());
                    }
                }
            }
        }
        let properties = &payload["properties"];
        match payload["type"].as_str().unwrap_or_default() {
            "message.updated" => {
                let info = &properties["info"];
                if info["sessionID"] != session_id || info["role"] != "assistant" {
                    return Ok(());
                }
                let message_id = required_string(info, "id")?;
                let parent = required_string(info, "parentID")?;
                if !self.assistant_messages.contains(message_id) {
                    if self.assistant_messages.len() >= MAX_TRACKED_PARTS {
                        return Err(io::Error::other(
                            "OpenCode assistant-message limit exceeded",
                        ));
                    }
                    // Kilo executes queued users individually. OpenCode may consume
                    // several intervening users in the same loop. The assistant's
                    // parent identifies the last input actually included in this step.
                    if let Some(index) = self.pending_users.iter().position(|(id, _)| id == parent)
                    {
                        for _ in 0..=index {
                            let (_, text) = self.pending_users.pop_front().expect("matched parent");
                            control.user_text(session_id, &text).await?;
                        }
                    }
                    self.assistant_messages.insert(message_id.to_owned());
                }
                if let Some(error) = info.get("error").filter(|error| !error.is_null()) {
                    self.error = Some(error.clone());
                }
            }
            "message.part.updated" => {
                let part = &properties["part"];
                if part["sessionID"] != session_id
                    || !part["messageID"]
                        .as_str()
                        .is_some_and(|id| self.assistant_messages.contains(id))
                {
                    return Ok(());
                }
                match part["type"].as_str().unwrap_or_default() {
                    // Kilo renders temporary UI progress as text parts. These
                    // are later removed and must never enter an answer stream.
                    // Synthetic alone does not mean temporary or non-answer.
                    "text"
                        if part["ignored"] != true
                            && part["metadata"]["kilocode.lifecycle"] != "transient" =>
                    {
                        let id = required_string(part, "id")?;
                        let text = required_string(part, "text")?;
                        if !self.text_parts.contains_key(id)
                            && self.text_parts.len() >= MAX_TRACKED_PARTS
                        {
                            return Err(io::Error::other("OpenCode text-part limit exceeded"));
                        }
                        let previous = self.text_parts.entry(id.to_owned()).or_default();
                        // Completion plugins may rewrite the stored text. As with
                        // ACP streaming, that replacement is not an additive delta
                        // and must neither duplicate output nor kill the session.
                        let delta = text.strip_prefix(previous.as_str()).unwrap_or_default();
                        if !delta.is_empty() {
                            control.session_update(session_id, json!({
                                "sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":delta}
                            })).await?;
                        }
                        *previous = text.to_owned();
                    }
                    "tool" => {
                        let state = &part["state"];
                        // Preserve the ACP tool projection, using the same ordered
                        // parts as the text instead of a second subscriber's timing.
                        control.session_update(session_id, json!({
                            "sessionUpdate":"tool_call_update", "toolCallId":part["callID"],
                            "title":part["tool"], "rawInput":state["input"], "status":state["status"],
                            "rawOutput":{"output":state["output"],"metadata":state["metadata"]}
                        })).await?;
                    }
                    _ => {}
                }
            }
            "message.part.delta" => {
                if properties["sessionID"] != session_id
                    || properties["field"] != "text"
                    || !properties["messageID"]
                        .as_str()
                        .is_some_and(|id| self.assistant_messages.contains(id))
                {
                    return Ok(());
                }
                let id = required_string(properties, "partID")?;
                // Reasoning, ignored, transient and non-text parts have no entry.
                if let Some(text) = self.text_parts.get_mut(id) {
                    let delta = required_string(properties, "delta")?;
                    text.push_str(delta);
                    control.session_update(session_id, json!({
                        "sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":delta}
                    })).await?;
                }
            }
            "session.error" if properties["sessionID"] == session_id => {
                self.error = properties
                    .get("error")
                    .filter(|error| !error.is_null())
                    .cloned();
            }
            "session.status" if properties["sessionID"] == session_id => {
                match properties["status"]["type"].as_str() {
                    Some("busy") => control.session_status(session_id, true, None).await?,
                    Some("idle") => {
                        control
                            .session_status(session_id, false, self.error.take())
                            .await?;
                        self.assistant_messages.clear();
                        self.text_parts.clear();
                        // Queued user messages survive the preceding turn's idle.
                    }
                    _ => {}
                }
            }
            "message.removed" if properties["sessionID"] == session_id => {
                if let Some(id) = properties["messageID"].as_str() {
                    self.pending_users.retain(|(pending, _)| pending != id);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn required_string<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| io::Error::other(format!("OpenCode event has no string {key}")))
}

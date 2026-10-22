use super::{AnalystEvent, Value, json};
use std::collections::BTreeMap;
use std::io;
use tokio::sync::{broadcast, oneshot};

const PROVIDER_FAILED: &str = "Pi provider turn failed; check the selected provider configuration";
const LENGTH_STOPPED: &str = concat!(
    "Pi stopped the turn at the model's length limit before a final reply; ",
    "the context window or output budget is used up"
);

pub(super) struct Turn {
    pub id: String,
    pub delegation_id: String,
    pub text: String,
    pub admitted: bool,
    pub provider_started: bool,
    pub cancelling: bool,
    pub admission: Option<oneshot::Sender<io::Result<String>>>,
    final_text: String,
    failure: Option<&'static str>,
    message: u64,
    tools: BTreeMap<String, Value>,
    activities: BTreeMap<String, String>,
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
            failure: None,
            message: 0,
            tools: BTreeMap::new(),
            activities: BTreeMap::new(),
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
            } else if self.failure.is_some() {
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
                self.failure = match record["message"]["stopReason"].as_str() {
                    Some("error" | "aborted") => Some(PROVIDER_FAILED),
                    // The model's context or output budget ran out before it
                    // wrote any text (a reasoning-only answer included). Pi
                    // settles this like a normal turn, so it would read as an
                    // idle, healthy Actor that answered nothing.
                    Some("length") if self.final_text.trim().is_empty() => Some(LENGTH_STOPPED),
                    _ => None,
                };
                publish(
                    sender,
                    generation,
                    None,
                    json!({"method":"item/completed","params":{"threadId":session,"turnId":self.id,"item":{"id":format!("pi-message-{}",self.message),"type":"agentMessage","text":self.final_text}}}),
                );
            }
            "auto_retry_end" if record["success"] == false => self.failure = Some(PROVIDER_FAILED),
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                let title = record["toolName"].as_str().unwrap_or_default();
                let id = record["toolCallId"].as_str().unwrap_or_default();
                let mut params = json!({"threadId":session,"turnId":self.id,"title":title.chars().take(400).collect::<String>()});
                // A start and its end name the call, so the journal can show one
                // started-then-finished activity. Streaming updates stay transient.
                let phase = match typ {
                    "tool_execution_start" => Some("started"),
                    "tool_execution_end" if record["isError"] == true => Some("failed"),
                    "tool_execution_end" => Some("completed"),
                    _ => None,
                };
                if let Some(phase) = phase
                    && !id.is_empty()
                {
                    let detail = if phase == "started" {
                        let detail = call_detail(title, &record["args"]);
                        self.activities.insert(id.into(), detail.clone());
                        detail
                    } else {
                        self.activities.remove(id).unwrap_or_default()
                    };
                    params["itemId"] = json!(id);
                    params["itemType"] = json!(item_type(title));
                    params["phase"] = json!(phase);
                    params["detail"] = json!(detail);
                }
                publish(
                    sender,
                    generation,
                    None,
                    json!({"method":"cccc/toolActivity","params":params}),
                );
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
        let reason = self.failure.unwrap_or(PROVIDER_FAILED);
        let error = (status == "failed").then(|| json!({"message": reason}));
        publish(
            sender,
            generation,
            None,
            json!({"method":"turn/completed","params":{"threadId":session,"turn":{"id":self.id,"status":status,"error":error}}}),
        );
    }
}

/// The same activity categories a Codex trace uses, so the Web labels them alike.
fn item_type(tool: &str) -> &'static str {
    match tool {
        "bash" => "commandExecution",
        "edit" | "write" => "fileChange",
        _ if tool.starts_with("mcp__") => "mcpToolCall",
        _ => "toolCall",
    }
}

/// What the journal shows of a call besides its tool name. The other adapters
/// publish tool names or titles only, never arguments; Pi adds just enough to
/// tell its own calls apart: the program `bash` runs (with the subcommand of a
/// few multi-command tools, as in `cargo test`) and the file name, without its
/// directory, that `read`, `write` or `edit` works on. Command arguments,
/// directories, patterns and every other tool's input, MCP tools included,
/// never reach the journal.
fn call_detail(tool: &str, args: &Value) -> String {
    const LIMIT: usize = 64;
    let detail = match tool {
        "bash" => args["command"].as_str().map(program).unwrap_or_default(),
        "read" | "write" | "edit" => args["path"]
            .as_str()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    };
    detail.chars().take(LIMIT).collect()
}

/// `cargo test` for `KEY=value cargo test --locked`, `deploy` for
/// `/opt/tools/deploy --target prod`: the program's file name, and for a few
/// tools whose first argument names a subcommand, that word.
fn program(command: &str) -> String {
    // shell-words handles quoting, not shell expansion or control syntax.
    // Keep complex commands out of summaries rather than misidentify a secret
    // inside an assignment/substitution as the executable.
    if command.contains(['$', '`', '(', ')', ';', '|', '&', '<', '>', '\n', '\r']) {
        return String::new();
    }
    const SUBCOMMANDS: [&str; 10] = [
        "cargo", "git", "npm", "pnpm", "yarn", "go", "docker", "kubectl", "uv", "gh",
    ];
    // Split quoted words before skipping shell assignments: whitespace inside
    // a credential must never make a fragment look like the executable name.
    let Ok(words) = shell_words::split(command) else {
        return String::new();
    };
    let mut words = words.iter().map(String::as_str).skip_while(|word| {
        word.split_once('=').is_some_and(|(name, _)| {
            let mut chars = name.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    });
    let Some(first) = words.next() else {
        return String::new();
    };
    let name = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let subcommand = words.next().filter(|word| {
        SUBCOMMANDS.contains(&name)
            && word.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            && !word.starts_with('-')
    });
    match subcommand {
        Some(word) => format!("{name} {word}"),
        None => name.to_owned(),
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

/// Real Pi 1.3.0 RPC output (`pi --mode rpc`, 2026-10-22, an OpenAI-compatible
/// provider) for one prompt that hit the model's length limit. The model's
/// configured `contextWindow` was set just above the prompt, so Pi clamped the
/// request's output budget to one token: the model spent it and wrote no text
/// (`content: []`, `stopReason: "length"`), Pi removed the empty reply from its
/// context (`context_edit`) and settled. Each line is as captured; only the two
/// system-prompt records and the closing `agent_end` transcript echo, which
/// carry local paths, are left out.
#[cfg(test)]
pub(crate) const CAPTURED_LENGTH_STOP: &str = include_str!("captured_length_stop_empty.jsonl");

/// Real Pi 1.3.0 RPC tool execution capture, 2026-10-21: bash lists a
/// scratch directory and read opens notes.txt. Local-path system-prompt
/// records and the closing agent_end transcript echo were omitted.
#[cfg(test)]
pub(crate) const CAPTURED_TOOL_TURN: &str = include_str!("captured_tool_turn.jsonl");

/// Replays captured Pi RPC records through one controlled turn and returns the
/// normalized events, in order. `response` records stay with the protocol
/// loop, as in a live session.
#[cfg(test)]
pub(crate) fn replay_captured_turn(captured: &str, thread_id: &str) -> Vec<Value> {
    let records = captured
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("captured Pi RPC record"))
        .filter(|record| record["type"] != "response")
        .collect::<Vec<_>>();
    let prompt = records
        .iter()
        .find(|record| record["type"] == "message_start" && record["message"]["role"] == "user")
        .map(|record| message_text(&record["message"]))
        .expect("captured prompt");
    let (sender, mut receiver) = broadcast::channel(records.len() * 4);
    let (admission, _) = oneshot::channel();
    let mut turn = Turn::new("captured".into(), prompt, admission);
    let mut settled = false;
    for record in &records {
        settled |= turn
            .observe(record, &sender, "generation", thread_id)
            .expect("captured record is accepted");
    }
    assert!(settled, "the captured session settles its turn");
    std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|event| event.message)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single turn/completed a replay publishes.
    fn settled_turn(events: &[Value]) -> Value {
        let terminals = events
            .iter()
            .filter(|event| event["method"] == "turn/completed")
            .collect::<Vec<_>>();
        assert_eq!(terminals.len(), 1, "one terminal per turn");
        terminals[0]["params"]["turn"].clone()
    }

    #[test]
    fn a_captured_length_stop_without_a_reply_settles_failed() {
        // The second capture is the reasoning-only variant: the model spent
        // its budget thinking (`content: [thinking]`) and wrote no text.
        for captured in [
            CAPTURED_LENGTH_STOP,
            include_str!("captured_length_stop_thinking.jsonl"),
        ] {
            let turn = settled_turn(&replay_captured_turn(captured, "s"));
            assert_eq!(turn["status"], "failed", "{turn}");
            assert!(
                turn["error"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("length limit")),
                "{turn}"
            );
        }
    }

    #[test]
    fn a_captured_length_stop_with_a_partial_reply_still_completes() {
        let events = replay_captured_turn(include_str!("captured_length_stop_partial.jsonl"), "s");
        let turn = settled_turn(&events);
        assert_eq!(turn["status"], "completed");
        assert_eq!(turn["error"], Value::Null);
        let answer = events
            .iter()
            .find(|event| {
                event["method"] == "item/completed"
                    && event["params"]["item"]["type"] == "agentMessage"
            })
            .expect("the partial reply is published");
        assert_eq!(
            answer["params"]["item"]["text"],
            "The quick brown fox jumps over the"
        );
    }

    #[test]
    fn captured_tool_calls_are_published_as_started_then_finished() {
        let calls = replay_captured_turn(CAPTURED_TOOL_TURN, "s")
            .into_iter()
            .filter(|message| {
                message["method"] == "cccc/toolActivity" && message["params"]["phase"].is_string()
            })
            .map(|message| {
                let params = &message["params"];
                [
                    params["phase"].clone(),
                    params["title"].clone(),
                    params["itemType"].clone(),
                    params["detail"].clone(),
                ]
            })
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                ["started", "bash", "commandExecution", "ls"],
                ["started", "read", "toolCall", "notes.txt"],
                ["completed", "bash", "commandExecution", "ls"],
                ["completed", "read", "toolCall", "notes.txt"],
            ]
            .map(|call| call.map(Value::from))
        );
    }

    /// A constructed privacy test, not a capture: a credential-like token and a
    /// private directory in every place a call can carry them. Only a program
    /// or a file name comes through.
    #[test]
    fn a_call_shows_only_its_program_or_file_name() {
        const TOKEN: &str = "secret-token-0123456789abcdef";
        const PRIVATE: &str = "/home/someone/private-dir";
        let calls = [
            (
                "bash",
                json!({"command": format!("curl -H 'Authorization: Bearer {TOKEN}' https://user:{TOKEN}@example.com{PRIVATE}")}),
                "curl",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY={TOKEN} cargo test --locked -p private-crate")}),
                "cargo test",
            ),
            (
                "bash",
                json!({"command": format!("{PRIVATE}/bin/deploy --token {TOKEN}")}),
                "deploy",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY='fixture {TOKEN}' cargo test --locked")}),
                "cargo test",
            ),
            (
                "bash",
                json!({"command": format!("'{PRIVATE}/tools with spaces/deploy' --token {TOKEN}")}),
                "deploy",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY='fixture {TOKEN} cargo test")}),
                "",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY=$(echo 'fixture {TOKEN}') cargo test")}),
                "",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY=`echo 'fixture {TOKEN}'` cargo test")}),
                "",
            ),
            (
                "bash",
                json!({"command": format!("API_KEY=<(echo 'fixture {TOKEN}') cargo test")}),
                "",
            ),
            (
                "bash",
                json!({"command": "API_KEY=$((1 + 2)) cargo test"}),
                "",
            ),
            ("bash", json!({"command": "cargo test && printf done"}), ""),
            ("bash", json!({"command": "cargo test > result.txt"}), ""),
            (
                "read",
                json!({"path": format!("{PRIVATE}/notes.txt")}),
                "notes.txt",
            ),
            (
                "write",
                json!({"path": format!("{PRIVATE}/report.md"), "content": TOKEN}),
                "report.md",
            ),
            (
                "edit",
                json!({"path": format!("{PRIVATE}/main.rs"), "edits": [{"oldText": TOKEN, "newText": "x"}]}),
                "main.rs",
            ),
            ("grep", json!({"pattern": TOKEN, "path": PRIVATE}), ""),
            ("find", json!({"pattern": "*.key", "path": PRIVATE}), ""),
            ("ls", json!({"path": PRIVATE}), ""),
            (
                "mcp__cccc__cccc_message_send",
                json!({"text": TOKEN, "to": "peer"}),
                "",
            ),
            ("mcp__search__web_search", json!({"query": TOKEN}), ""),
            (
                "web_fetch",
                json!({"url": format!("https://example.com{PRIVATE}?token={TOKEN}")}),
                "",
            ),
        ];
        let (sender, mut receiver) = broadcast::channel(32);
        let (admission, _) = oneshot::channel();
        let mut turn = Turn::new("d".into(), "exact".into(), admission);
        turn.observe(
            &json!({"type":"message_start","message":{"role":"user","content":"exact"}}),
            &sender,
            "g",
            "s",
        )
        .expect("admitted");
        for (index, (tool, args, _)) in calls.iter().enumerate() {
            let record = json!({
                "type": "tool_execution_start",
                "toolCallId": format!("call-{index}"),
                "toolName": tool,
                "args": args,
            });
            turn.observe(&record, &sender, "g", "s").expect("accepted");
        }
        let published = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|event| event.message)
            .filter(|message| message["method"] == "cccc/toolActivity")
            .collect::<Vec<_>>();
        assert_eq!(published.len(), calls.len());
        for (message, (tool, _, shown)) in published.iter().zip(&calls) {
            assert_eq!(message["params"]["title"], *tool);
            assert_eq!(message["params"]["detail"], *shown, "{message}");
            let text = message.to_string();
            assert!(!text.contains(TOKEN), "{text}");
            assert!(!text.contains("private"), "{text}");
        }
    }

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

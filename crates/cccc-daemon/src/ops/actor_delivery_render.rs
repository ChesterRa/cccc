use cccc_contracts::Event;
use serde_json::Value;

mod system_notify;

fn mcp_reply_reminder() -> String {
    format!(
        "[cccc] {}",
        cccc_core::system_prompt::MESSAGE_DELIVERY_GUIDANCE
    )
}

fn render_message(event: &Event) -> Option<String> {
    if event.kind == "system.notify" {
        return render_system(event);
    }
    let mut body = event
        .data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    let mut protocol = protocol_lines(event);
    protocol.extend(reference_lines(event));
    protocol.extend(attachment_lines(event));
    if !protocol.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&protocol.join("\n"));
    }
    body = cccc_core::peer_insight::append_to_delivery(&body, event.data.get("insight"));
    if body.is_empty() {
        return None;
    }
    Some(format_envelope(event, &body))
}

pub fn render_batch(events: &[Event]) -> Option<String> {
    let messages = events
        .iter()
        .map(render_message)
        .collect::<Option<Vec<_>>>()?;
    let rendered = match messages.as_slice() {
        [] => None,
        [message] => Some(message.clone()),
        _ => Some(format!(
            "[cccc] {} new messages:\n\n{}",
            messages.len(),
            messages.join("\n\n")
        )),
    }?;
    Some(if events.iter().any(|event| event.kind == "chat.message") {
        append_mcp_reply_reminder(&rendered)
    } else {
        rendered
    })
}

fn append_mcp_reply_reminder(text: &str) -> String {
    let out = text.trim_end_matches(['\r', '\n']);
    let reminder = mcp_reply_reminder();
    if out.contains(&reminder) {
        return out.to_owned();
    }
    if out.is_empty() {
        reminder
    } else {
        format!("{out}\n\n{reminder}")
    }
}

fn protocol_lines(event: &Event) -> Vec<String> {
    let mut lines = Vec::new();
    if text(event, "priority") == "attention" {
        lines.push(format!("[cccc] IMPORTANT (event_id={}):", event.id));
    }
    if event
        .data
        .get("reply_required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        lines.push(format!(
            "[cccc] REPLY REQUIRED (event_id={}): reply via cccc_message_reply.",
            event.id
        ));
    }
    let source_group = text(event, "src_group_id");
    let source_event = text(event, "src_event_id");
    if !source_group.is_empty() && !source_event.is_empty() {
        lines.push(format!(
            "[cccc] RELAYED FROM (group_id={source_group}, event_id={source_event}):"
        ));
    }
    let remote_reply_to = strings(event, "remote_reply_to");
    if !remote_reply_to.is_empty() {
        lines.push(format!(
            "[cccc] REMOTE REPLY DEFAULT: omit to in cccc_message_reply to reply to remote {}.",
            remote_reply_to.join(", ")
        ));
    }
    lines
}

fn reference_lines(event: &Event) -> Vec<String> {
    let refs = event
        .data
        .get("refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("hidden").and_then(Value::as_bool) != Some(true))
        .take(4)
        .collect::<Vec<_>>();
    if refs.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["[cccc] References:".to_owned()];
    for item in refs {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("ref");
        let label = ["title", "path", "url", "task_id", "slot_id"]
            .into_iter()
            .find_map(|key| item.get(key).and_then(Value::as_str))
            .unwrap_or(kind);
        lines.push(format!("- {kind}: {}", compact(label, 120)));
    }
    lines
}

fn attachment_lines(event: &Event) -> Vec<String> {
    let attachments = event
        .data
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
        .collect::<Vec<_>>();
    if attachments.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "[cccc] Attachments: use cccc_file(action=\"read\", group_id=\"{}\", rel_path=...) for text; use action=\"blob_path\" for images/binary files.",
        event.group_id
    )];
    for item in attachments {
        let path = item.get("path").and_then(Value::as_str).unwrap_or_default();
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(path);
        let bytes = item.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        lines.push(format!(
            "- {} ({bytes} bytes) [{path}]",
            compact(title, 120)
        ));
    }
    lines
}

fn format_envelope(event: &Event, body: &str) -> String {
    let source = ["source_platform", "source_user_name", "source_user_id"]
        .into_iter()
        .map(|key| text(event, key))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let sender = if source.is_empty() {
        event.by.clone()
    } else {
        format!("{}[{}]", event.by, source.join(" / "))
    };
    let targets = strings(event, "to");
    let targets = if targets.is_empty() {
        "@all".to_owned()
    } else {
        targets.join(", ")
    };
    let reply_to = text(event, "reply_to");
    let reply = if reply_to.is_empty() {
        String::new()
    } else {
        format!(" (reply:{})", reply_to.chars().take(8).collect::<String>())
    };
    let quote = compact(&text(event, "quote_text").replace(['\r', '\n'], " "), 80);
    let quote = if quote.is_empty() {
        String::new()
    } else {
        format!("\n> \"{quote}\"")
    };
    if body.contains(['\r', '\n']) {
        format!("[cccc] {sender} → {targets}{reply}{quote}:\n{body}")
    } else {
        format!("[cccc] {sender} → {targets}{reply}{quote}: {body}")
    }
}

fn render_system(event: &Event) -> Option<String> {
    let kind = text(event, "kind");
    let body = system_notify::body(event);
    (!body.is_empty()).then(|| {
        format!(
            "[cccc] SYSTEM ({}): {body}",
            if kind.is_empty() { "info" } else { &kind }
        )
    })
}

fn text(event: &Event, key: &str) -> String {
    event
        .data
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn strings(event: &Event, key: &str) -> Vec<String> {
    event
        .data
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn compact(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        return normalized;
    }
    format!(
        "{}...",
        normalized
            .chars()
            .take(limit.saturating_sub(3))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_complete_delivery_contract() {
        let mut event = Event::new("chat.message", "g_test");
        event.id = "event-123".into();
        event.by = "user".into();
        event.data = json!({
            "to":["peer1"], "text":"inspect",
            "insight":"The dependency boundary matters more than the local patch.",
            "priority":"attention", "reply_required":true,
            "refs":[{"kind":"task_ref","task_id":"task-1","title":"Fix send"}],
            "attachments":[{"path":"state/blobs/abc","title":"screen.png","bytes":42}]
        })
        .as_object()
        .cloned()
        .expect("object");
        let rendered = render_batch(&[event]).expect("render");
        assert!(rendered.contains("IMPORTANT (event_id=event-123)"));
        assert!(rendered.contains("REPLY REQUIRED (event_id=event-123)"));
        assert!(rendered.contains("task_ref: Fix send"));
        assert!(rendered.contains("cccc_file(action=\"read\", group_id=\"g_test\""));
        assert!(rendered.contains("screen.png (42 bytes) [state/blobs/abc]"));
        assert!(rendered.contains(cccc_core::peer_insight::PEER_PERSPECTIVE_AGENT_LABEL));
        assert!(rendered.contains("dependency boundary matters"));
    }

    #[test]
    fn renders_multiple_events_as_one_delivery_batch() {
        let mut first = Event::new("chat.message", "g_test");
        first.by = "reviewer".into();
        first.data = json!({"to":["lead"],"text":"first"})
            .as_object()
            .cloned()
            .expect("event data");
        let mut second = Event::new("chat.message", "g_test");
        second.by = "backend".into();
        second.data = json!({"to":["lead"],"text":"second"})
            .as_object()
            .cloned()
            .expect("event data");

        let rendered = render_batch(&[first, second]).expect("batch");
        assert!(rendered.starts_with("[cccc] 2 new messages:"));
        assert!(rendered.contains("[cccc] reviewer → lead: first"));
        assert!(rendered.contains("[cccc] backend → lead: second"));
        assert_eq!(rendered.matches(&mcp_reply_reminder()).count(), 1);
    }

    #[test]
    fn appends_python_compatible_mcp_reminder_to_each_chat_delivery() {
        let mut event = Event::new("chat.message", "g_test");
        event.by = "user".into();
        event.data = json!({"to":["codex-1"], "text":"你好"})
            .as_object()
            .cloned()
            .expect("object");

        assert_eq!(
            render_batch(&[event]).expect("rendered"),
            format!("[cccc] user → codex-1: 你好\n\n{}", mcp_reply_reminder())
        );
    }

    #[test]
    fn renders_voice_secretary_input_envelope_in_full() {
        let mut event = Event::new("system.notify", "g_test");
        event.data = json!({
            "context": {
                "kind": "voice_secretary_input",
                "input_envelope": {
                    "text": "整理本次会议结论",
                    "session_id": "voice-session-1",
                    "segment_id": "segment-7",
                    "speaker": "user"
                }
            }
        })
        .as_object()
        .cloned()
        .expect("object");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("daemon-delivered input_envelope"));
        assert!(rendered.contains("整理本次会议结论"));
        assert!(rendered.contains("\"session_id\": \"voice-session-1\""));
        assert!(rendered.contains("\"segment_id\": \"segment-7\""));
    }

    #[test]
    fn renders_voice_secretary_action_request_envelope() {
        let mut event = Event::new("system.notify", "g_test");
        event.data = json!({
            "context": {
                "kind": "voice_secretary_action_request",
                "request": {
                    "request_id": "request-9",
                    "document_path": "voice/meeting.md",
                    "request_text": "生成行动项并发给项目组",
                    "priority": "attention"
                }
            }
        })
        .as_object()
        .cloned()
        .expect("object");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("kind=voice_secretary_action_request"));
        assert!(rendered.contains("request_id=request-9"));
        assert!(rendered.contains("document_path=voice/meeting.md"));
        assert!(rendered.contains("生成行动项并发给项目组"));
        assert!(rendered.contains("\"priority\": \"attention\""));
    }
}

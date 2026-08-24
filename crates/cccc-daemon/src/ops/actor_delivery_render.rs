use cccc_contracts::Event;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::Value;

mod references;
mod system_notify;

fn render_message(event: &Event) -> Option<String> {
    if event.kind == "system.notify" {
        return render_system(event);
    }
    let mut body = super::messaging::install_command::delivery_text(event).unwrap_or_else(|| {
        event
            .data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    });
    let mut protocol = protocol_lines(event);
    protocol.extend(references::lines(event));
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
    match messages.as_slice() {
        [] => None,
        [message] => Some(message.clone()),
        _ => Some(format!(
            "[cccc] {} new messages:\n\n{}",
            messages.len(),
            messages.join("\n\n")
        )),
    }
}

pub fn render_batch_with_mail_context(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    events: &[Event],
) -> Option<String> {
    let mut output = render_batch(events)?;
    let has_direct_message = events.iter().any(|event| {
        event.kind == "chat.message"
            && matches!(
                event.data.get("message_mode").and_then(Value::as_str),
                Some("send" | "request_reply")
            )
    });
    if !has_direct_message {
        return Some(output);
    }
    let pending = cccc_core::inbox::mail_pending_summary(home, group, actor_id)
        .ok()
        .flatten();
    let count = pending
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if count > 0 {
        let noun = if count == 1 { "item" } else { "items" };
        output.push_str(&format!(
            "\n\n[cccc] MAIL PENDING: {count} {noun}. Call cccc_inbox_read when appropriate."
        ));
    }
    Some(output)
}

fn protocol_lines(event: &Event) -> Vec<String> {
    let mut lines = Vec::new();
    if text(event, "message_mode") == "request_reply" {
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
    let message_mode = match text(event, "message_mode") {
        value if !value.is_empty() => value,
        _ => "send".to_owned(),
    };
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
    let metadata = if event.id.trim().is_empty() {
        String::new()
    } else {
        let parent = if reply_to.is_empty() {
            String::new()
        } else {
            format!(" reply_to={reply_to}")
        };
        format!(
            " [event_id={} message_mode={message_mode}{parent}]",
            event.id
        )
    };
    if body.contains(['\r', '\n']) {
        format!("[cccc] {sender} → {targets}{reply}{metadata}{quote}:\n{body}")
    } else {
        format!("[cccc] {sender} → {targets}{reply}{metadata}{quote}: {body}")
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
            "message_mode":"request_reply",
            "refs":[{"kind":"task_ref","task_id":"task-1","title":"Fix send"}],
            "attachments":[{"path":"state/blobs/abc","title":"screen.png","bytes":42}]
        })
        .as_object()
        .cloned()
        .expect("object");
        let rendered = render_batch(&[event]).expect("render");
        assert!(
            rendered.starts_with(
                "[cccc] user → peer1 [event_id=event-123 message_mode=request_reply]:\n"
            )
        );
        assert!(
            rendered.contains("REPLY REQUIRED (event_id=event-123): reply via cccc_message_reply.")
        );
        assert!(rendered.contains("task_ref: Fix send"));
        assert!(rendered.contains("cccc_file(action=\"read\", group_id=\"g_test\""));
        assert!(rendered.contains("screen.png (42 bytes) [state/blobs/abc]"));
        assert!(rendered.contains(cccc_core::peer_insight::PEER_PERSPECTIVE_AGENT_LABEL));
        assert!(rendered.contains("dependency boundary matters"));
    }

    #[test]
    fn direct_delivery_adds_mail_count_without_consuming_mail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("mail context", "").expect("group");
        group.actors.push(cccc_contracts::Actor::new("peer1"));
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");

        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.data = json!({
            "to":["peer1"],
            "text":"read later",
            "message_mode":"mail",
        })
        .as_object()
        .cloned()
        .expect("mail data");
        cccc_core::ledger::append(&ledger_path, &mail).expect("append mail");

        let mut direct = Event::new("chat.message", &group.group_id);
        direct.by = "user".into();
        direct.data = json!({
            "to":["peer1"],
            "text":"look now",
            "message_mode":"send",
        })
        .as_object()
        .cloned()
        .expect("direct data");

        let rendered = render_batch_with_mail_context(&home, &group, "peer1", &[direct])
            .expect("render direct");
        assert!(rendered.contains("MAIL PENDING: 1 item"));
        assert_eq!(
            cccc_core::inbox::list_unread(&home, &group, "peer1", 10)
                .expect("unread")
                .len(),
            1
        );

        let rendered_mail =
            render_batch_with_mail_context(&home, &group, "peer1", &[mail]).expect("render mail");
        assert!(!rendered_mail.contains("MAIL PENDING"));
    }

    #[test]
    fn renders_remote_reply_default_with_reply_tool() {
        let mut event = Event::new("chat.message", "g_test");
        event.by = "remote-peer".into();
        event.data = json!({
            "to":["lead"],
            "text":"remote message",
            "remote_reply_to":["group-a/actor-1", "group-b/actor-2"]
        })
        .as_object()
        .cloned()
        .expect("event data");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains(
            "REMOTE REPLY DEFAULT: omit to in cccc_message_reply to reply to remote group-a/actor-1, group-b/actor-2."
        ));
        assert!(!rendered.contains("omit to in cccc_message_send"));
    }

    #[test]
    fn renders_multiple_events_as_one_delivery_batch() {
        let mut first = Event::new("chat.message", "g_test");
        first.id = "event-first".into();
        first.by = "reviewer".into();
        first.data = json!({"to":["lead"],"text":"first"})
            .as_object()
            .cloned()
            .expect("event data");
        let mut second = Event::new("chat.message", "g_test");
        second.id = "event-second".into();
        second.by = "backend".into();
        second.data = json!({"to":["lead"],"text":"second"})
            .as_object()
            .cloned()
            .expect("event data");

        let rendered = render_batch(&[first, second]).expect("batch");
        assert!(rendered.starts_with("[cccc] 2 new messages:"));
        assert!(
            rendered
                .contains("[cccc] reviewer → lead [event_id=event-first message_mode=send]: first")
        );
        assert!(
            rendered.contains(
                "[cccc] backend → lead [event_id=event-second message_mode=send]: second"
            )
        );
        assert!(!rendered.contains(cccc_core::system_prompt::MESSAGE_DELIVERY_GUIDANCE));
    }

    #[test]
    fn does_not_repeat_system_prompt_guidance_in_each_chat_delivery() {
        let mut event = Event::new("chat.message", "g_test");
        event.id = "event-plain".into();
        event.by = "user".into();
        event.data = json!({"to":["codex-1"], "text":"你好"})
            .as_object()
            .cloned()
            .expect("object");

        let rendered = render_batch(&[event]).expect("rendered");
        assert_eq!(
            rendered,
            "[cccc] user → codex-1 [event_id=event-plain message_mode=send]: 你好"
        );
    }

    #[test]
    fn keeps_current_event_and_parent_reply_id_distinct() {
        let mut event = Event::new("chat.message", "g_test");
        event.id = "event-current".into();
        event.by = "peer2".into();
        event.data = json!({
            "to":["peer1"],
            "text":"follow-up",
            "message_mode":"send",
            "reply_to":"event-parent"
        })
        .as_object()
        .cloned()
        .expect("event data");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.starts_with(
            "[cccc] peer2 → peer1 (reply:event-pa) [event_id=event-current message_mode=send reply_to=event-parent]: follow-up"
        ));
    }

    #[test]
    fn delivers_hidden_slash_skill_control_text_without_exposing_its_ref() {
        let mut event = Event::new("chat.message", "g_test");
        event.by = "user".into();
        event.data = json!({
            "to":["architect"],
            "text":"[CCCC] INTERNAL CONTROL: CCCC capability skill dispatch\n\nskill_command: /using-superpowers\n\ncapability_id: skill:test:using-superpowers\n\nUser task:\n开始执行",
            "refs":[{
                "kind":"text",
                "title":"slash_skill_dispatch",
                "hidden":true,
                "control_kind":"slash_skill_dispatch",
                "command":"/using-superpowers",
                "capability_id":"skill:test:using-superpowers",
                "task_text":"开始执行"
            }]
        })
        .as_object()
        .cloned()
        .expect("event data");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("[CCCC] INTERNAL CONTROL"));
        assert!(rendered.contains("skill_command: /using-superpowers"));
        assert!(rendered.contains("capability_id: skill:test:using-superpowers"));
        assert!(rendered.contains("User task:\n开始执行"));
        assert!(!rendered.contains("[cccc] References:"));
    }

    #[test]
    fn delivers_install_as_capability_control_while_ledger_text_stays_user_facing() {
        let mut event = Event::new("chat.message", "g_test");
        event.by = "user".into();
        event.data = json!({
            "to":["architect"],
            "text":"/install owner/repo",
            "refs":[{
                "kind":"text",
                "title":"slash_command",
                "command":"/install",
                "capability_id":"skill:cccc:install",
                "args_text":"owner/repo",
                "target":"owner/repo",
                "target_kind":"repo_slug"
            }]
        })
        .as_object()
        .cloned()
        .expect("event data");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("[cccc] Slash command: /install"));
        assert!(rendered.contains("skill:cccc:install"));
        assert!(rendered.contains("cccc_capability_install"));
        assert!(rendered.contains("owner/repo"));
        assert!(!rendered.contains("]: /install owner/repo"));
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
    fn renders_voice_ask_with_an_explicit_report_contract() {
        let mut event = Event::new("system.notify", "g_test");
        event.data = json!({
            "context": {
                "kind": "voice_secretary_input",
                "input_envelope": {
                    "kind":"voice_instruction",
                    "request_id":"voice-ask-weather",
                    "text":"Task:\n厦门天气怎么样？",
                    "metadata":{"target_kind":"secretary"}
                }
            }
        })
        .as_object()
        .cloned()
        .expect("object");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("Work order:"));
        assert!(rendered.contains("Target: secretary"));
        assert!(rendered.contains("Request id: voice-ask-weather"));
        assert!(rendered.contains("Required output:"));
        assert!(rendered.contains(
            "cccc_voice_secretary_request(action=\"report\", request_id=\"voice-ask-weather\""
        ));
        assert!(rendered.contains("Console text alone is not delivered to the user."));
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
                    "priority": "high"
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
        assert!(rendered.contains("\"priority\": \"high\""));
    }

    #[test]
    fn voice_secretary_action_request_skips_empty_request_text_alias() {
        let mut event = Event::new("system.notify", "g_test");
        event.data = json!({
            "text": "event fallback",
            "context": {
                "kind": "voice_secretary_action_request",
                "request": {"request_text": "", "text": "legacy request text"}
            }
        })
        .as_object()
        .cloned()
        .expect("object");

        let rendered = render_batch(&[event]).expect("rendered");
        assert!(rendered.contains("legacy request text"));
        assert!(!rendered.contains("Request:\nevent fallback"));
    }
}

use cccc_contracts::{Actor, ActorRole, Event};
use cccc_core::{GroupDoc, monitor_records};
use serde_json::{Value, json};

// Claude Code 2.1.310 exposes only the first 500 raw characters of a Monitor
// stdout record. Budget the complete JSON line in bytes, including escaping,
// leaving room for the newline. The ledger remains the full-content authority.
const MONITOR_LINE_BYTES: usize = 480;
const PREVIEW_CHARS: usize = 80;

/// The Monitor notice, and whether it carries `instructions`. An ordinary
/// message from a member arrives whole as `rendered`. Otherwise a short notice
/// asks for the exact read: a line naming changed instruction files goes before
/// the preview, which is the part trimmed to the budget, and without room for it
/// the notice goes out without it. `preamble_due` drops the preview too, because
/// only the exact read hands back the instructions.
pub(crate) fn claude_notice(
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    instructions: Option<&str>,
    rendered: &str,
    preamble_due: bool,
) -> Option<(String, bool)> {
    let lookup = json!({"group_id":group.group_id,"event_id":event.id});
    let role = match cccc_core::actors::effective_role(group, &actor.id) {
        Some(ActorRole::Foreman) => "foreman",
        _ => "peer",
    };
    let scope = cccc_core::group_scope::effective_scope(group, Some(actor));
    let scope_key = scope.map_or("none", |scope| scope.scope_key.as_str());
    let mode = event
        .data
        .get("message_mode")
        .and_then(Value::as_str)
        .unwrap_or("notify");
    let context = format!(
        "You are {} ({role}); scope={scope_key}; from={}; mode={mode}.\n",
        actor.id, event.by,
    );
    let mut text = format!(
        "[CCCC] Read full message and instructions: cccc_message_history({lookup}).\n{context}"
    );
    if encoded_len(&text) > MONITOR_LINE_BYTES {
        return None;
    }
    if !preamble_due && arrives_whole(group, event) {
        let line = instructions.map_or_else(String::new, |line| format!("{line}\n"));
        let whole = format!(
            "[CCCC] Message below. If it ends in ...(truncated), read it in full before acting: cccc_message_history({lookup}). The same read returns the exact text and your instructions.\n{context}{line}{rendered}"
        );
        if monitor_records::fits_one_notice(&whole) {
            return Some((whole, instructions.is_some()));
        }
    }
    let mut included = false;
    if let Some(line) = instructions {
        let with_line = format!("{text}{line}\n");
        if encoded_len(&with_line) <= MONITOR_LINE_BYTES {
            text = with_line;
            included = true;
        }
    }
    let header_len = text.len();
    // System notices may carry their body as `message` instead of `text`.
    let preview = ["text", "message"]
        .into_iter()
        .find_map(|key| event.data.get(key).and_then(Value::as_str))
        .unwrap_or_default();
    if !preamble_due {
        text.extend(preview.chars().take(PREVIEW_CHARS));
    }
    while encoded_len(&text) > MONITOR_LINE_BYTES && text.len() > header_len {
        text.pop();
    }
    Some((text, included))
}

/// Claude Code tells the model that a Monitor event is not user input, so what
/// the user, Chet or a remote sender says is read from the ledger. Mail waits in
/// the Inbox. A message with files, references or a project path of its own needs
/// more than its rendered text says. Claude Code escapes `<`, `>` and `&` in a
/// Monitor event, so text with those stays literal by being read.
fn arrives_whole(group: &GroupDoc, event: &Event) -> bool {
    let empty = |key| {
        event
            .data
            .get(key)
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
    };
    let pathless = event
        .data
        .get("path")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    let literal = |key| {
        event
            .data
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|text| !text.contains(['<', '>', '&']))
    };
    event.kind == "chat.message"
        && cccc_core::actors::find(group, &event.by).is_some()
        && event.scope_key.is_empty()
        && pathless
        && literal("text")
        && literal("insight")
        && matches!(
            event.data.get("message_mode").and_then(Value::as_str),
            Some("send" | "request_reply")
        )
        && empty("attachments")
        && empty("refs")
}

fn encoded_len(text: &str) -> usize {
    serde_json::to_vec(&json!({"message":text}))
        .expect("notification strings serialize")
        .len()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::{GroupStore, HomeLayout, Scope};

    #[test]
    fn claude_monitor_record_keeps_exact_lookup_and_receiving_identity_visible() {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create(&"long title".repeat(300), "").expect("group");
        let actor = Actor::new("界".repeat(32));
        group.actors.push(actor.clone());
        group.scopes.push(Scope {
            scope_key: "s_fixture".into(),
            url: "/very/long/project/path".repeat(200),
            label: String::new(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_fixture".into();
        for body in ["short", &"\"\\\n界😀".repeat(2_000)] {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = json!({"text":body,"to":[actor.id],"message_mode":"request_reply"})
                .as_object()
                .expect("data")
                .clone();
            let (notice, included) =
                claude_notice(&group, &actor, &event, None, "unused", false).expect("notice");
            assert!(!included);
            let line = serde_json::to_string(&json!({"message":notice})).expect("JSON");
            assert!(
                line.len() < 500,
                "complete line must fit, even with JSON escaping"
            );
            assert!(notice.contains(&event.id));
            assert!(notice.contains(&group.group_id));
            assert!(notice.contains("cccc_message_history("));
            assert!(notice.contains(&format!("You are {} (foreman)", actor.id)));
            assert!(notice.contains("scope=s_fixture"));
            assert!(notice.contains("mode=request_reply"));
            assert!(!notice.contains("long title"));
            assert!(!notice.contains("very/long"));
            assert_eq!(event.data["text"], body, "ledger body stays complete");
        }
    }

    #[test]
    fn an_instruction_line_precedes_the_preview_or_waits_when_it_does_not_fit() {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("g", "").expect("group");
        let actor = Actor::new("界".repeat(32));
        group.actors.push(actor.clone());
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"text":"界".repeat(500),"to":[actor.id],"message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        let line = "[CCCC] AGENTS.md changed since CCCC began monitoring it; check current project instructions before acting.";
        let (notice, included) =
            claude_notice(&group, &actor, &event, Some(line), "unused", false).expect("notice");
        assert!(included);
        assert!(
            notice.contains(&format!("mode=send.\n{line}\n界")),
            "{notice}"
        );
        assert!(encoded_len(&notice) <= MONITOR_LINE_BYTES);

        let long = format!("[CCCC] {}", "x".repeat(400));
        let (notice, included) =
            claude_notice(&group, &actor, &event, Some(&long), "unused", false).expect("notice");
        assert!(!included, "the line waits; the message still goes out");
        assert!(!notice.contains("xxx"));
        assert!(notice.contains("mode=send.\n界"), "{notice}");
    }

    fn member_message(data: Value) -> (GroupDoc, Actor, Event, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut group = GroupStore::new(home)
            .expect("store")
            .create("g", "")
            .expect("group");
        let actor = Actor::new("receiver");
        group.actors.push(actor.clone());
        group.actors.push(Actor::new("sender"));
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "sender".into();
        event.data = data.as_object().expect("data").clone();
        (group, actor, event, temp)
    }

    const RENDERED: &str = "[cccc] sender -> receiver\nthe whole message";

    #[test]
    fn a_member_message_arrives_whole_after_its_routing_context() {
        for mode in ["send", "request_reply"] {
            let (group, actor, event, _temp) =
                member_message(json!({"text":"hello","to":["receiver"],"message_mode":mode}));
            let (notice, included) =
                claude_notice(&group, &actor, &event, None, RENDERED, false).expect("notice");
            assert!(!included);
            assert!(notice.starts_with("[CCCC] Message below."), "{notice}");
            assert!(notice.contains(&event.id));
            assert!(notice.contains(&group.group_id));
            assert!(notice.contains("cccc_message_history("));
            assert!(notice.contains("You are receiver ("));
            assert!(notice.ends_with(RENDERED), "{notice}");
        }
    }

    #[test]
    fn an_instruction_line_comes_before_a_whole_message() {
        let (group, actor, event, _temp) =
            member_message(json!({"text":"hello","message_mode":"send"}));
        let line = "[CCCC] AGENTS.md changed.";
        let (notice, included) =
            claude_notice(&group, &actor, &event, Some(line), RENDERED, false).expect("notice");
        assert!(included);
        assert!(notice.ends_with(&format!("{line}\n{RENDERED}")), "{notice}");
    }

    #[test]
    fn anything_but_a_plain_member_message_is_read_from_the_ledger() {
        let plain = json!({"text":"hello","message_mode":"send"});
        let cases = [
            ("user", plain.clone()),
            ("former-member", plain.clone()),
            ("connect:instance", plain.clone()),
            (cccc_core::actors::CHET_SERVICE_PRINCIPAL, plain.clone()),
            ("sender", json!({"text":"hello","message_mode":"mail"})),
            ("sender", json!({"text":"hello"})),
            (
                "sender",
                json!({"text":"hello","message_mode":"send","attachments":[{"path":"a"}]}),
            ),
            (
                "sender",
                json!({"text":"hello","message_mode":"send","refs":[{"kind":"ref"}]}),
            ),
            (
                "sender",
                json!({"text":"hello","message_mode":"send","path":"crates"}),
            ),
            (
                "sender",
                json!({"text":"hello","message_mode":"send","insight":"a && b"}),
            ),
        ];
        for (by, data) in cases {
            let (group, actor, mut event, _temp) = member_message(data);
            event.by = by.into();
            let (notice, _) =
                claude_notice(&group, &actor, &event, None, RENDERED, false).expect("notice");
            assert!(
                notice.starts_with("[CCCC] Read full message and instructions:"),
                "{by}: {notice}"
            );
            assert!(!notice.contains(RENDERED), "{by}: {notice}");
            assert!(notice.ends_with("hello"), "{by}: {notice}");
        }
    }

    #[test]
    fn text_that_claude_code_would_escape_is_read_from_the_ledger() {
        for text in ["run a > b", "Vec<String>", "a && b"] {
            let (group, actor, event, _temp) =
                member_message(json!({"text":text,"message_mode":"send"}));
            let (notice, _) =
                claude_notice(&group, &actor, &event, None, RENDERED, false).expect("notice");
            assert!(
                notice.starts_with("[CCCC] Read full message and instructions:"),
                "{notice}"
            );
        }
    }

    #[test]
    fn a_message_from_another_project_scope_is_read_from_the_ledger() {
        let (group, actor, mut event, _temp) =
            member_message(json!({"text":"hello","message_mode":"send"}));
        event.scope_key = "s_other".into();
        let (notice, _) =
            claude_notice(&group, &actor, &event, None, RENDERED, false).expect("notice");
        assert!(notice.starts_with("[CCCC] Read full message and instructions:"));
    }

    #[test]
    fn a_message_too_long_for_one_notification_asks_for_the_read() {
        let (group, actor, event, _temp) =
            member_message(json!({"text":"hello","message_mode":"send"}));
        let (notice, _) = claude_notice(&group, &actor, &event, None, &"界".repeat(3_000), false)
            .expect("notice");
        assert!(notice.starts_with("[CCCC] Read full message and instructions:"));
        assert!(encoded_len(&notice) <= MONITOR_LINE_BYTES);
    }

    #[test]
    fn a_due_preamble_withholds_the_message_until_the_read() {
        let (group, actor, event, _temp) =
            member_message(json!({"text":"hello","message_mode":"send"}));
        let line = "[CCCC] AGENTS.md changed.";
        let (notice, included) =
            claude_notice(&group, &actor, &event, Some(line), RENDERED, true).expect("notice");
        assert!(included);
        assert!(notice.starts_with("[CCCC] Read full message and instructions:"));
        assert!(notice.ends_with(&format!("{line}\n")), "{notice}");
        assert!(!notice.contains("hello"), "{notice}");
    }

    #[test]
    fn a_system_notice_without_a_mode_or_text_previews_its_message() {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut group = GroupStore::new(home)
            .expect("store")
            .create("g", "")
            .expect("group");
        let actor = Actor::new("x");
        group.actors.push(actor.clone());
        let mut event = Event::new("system.notify", &group.group_id);
        event.by = "system".into();
        event.data = json!({"kind":"runtime_switch_handoff","target_actor_id":"x","message":"You are now x."})
            .as_object()
            .expect("data")
            .clone();

        let (notice, _) =
            claude_notice(&group, &actor, &event, None, "unused", false).expect("notice");

        assert!(notice.contains("mode=notify.\nYou are now x."), "{notice}");
    }
}

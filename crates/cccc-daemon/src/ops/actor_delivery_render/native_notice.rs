use cccc_contracts::{Actor, ActorRole, Event};
use cccc_core::GroupDoc;
use serde_json::{Value, json};

// Claude Code 2.1.310 exposes only the first 500 raw characters of a Monitor
// stdout record. Budget the complete JSON line in bytes, including escaping,
// leaving room for the newline. The ledger remains the full-content authority.
const MONITOR_LINE_BYTES: usize = 480;

/// The Monitor notice, and whether it carries `instructions`: a line naming
/// changed instruction files goes before the preview, which is the part trimmed
/// to the budget. Without room for it, the notice goes out without it.
pub(crate) fn claude_notice(
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    instructions: Option<&str>,
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
    let mut text = format!(
        "[CCCC] Read full message and instructions: cccc_message_history({lookup}).\nYou are {} ({role}); scope={scope_key}; from={}; mode={mode}.\n",
        actor.id, event.by,
    );
    if encoded_len(&text) > MONITOR_LINE_BYTES {
        return None;
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
    text.extend(preview.chars().take(80));
    while encoded_len(&text) > MONITOR_LINE_BYTES && text.len() > header_len {
        text.pop();
    }
    Some((text, included))
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
            let (notice, included) = claude_notice(&group, &actor, &event, None).expect("notice");
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
        let (notice, included) = claude_notice(&group, &actor, &event, Some(line)).expect("notice");
        assert!(included);
        assert!(
            notice.contains(&format!("mode=send.\n{line}\n界")),
            "{notice}"
        );
        assert!(encoded_len(&notice) <= MONITOR_LINE_BYTES);

        let long = format!("[CCCC] {}", "x".repeat(400));
        let (notice, included) =
            claude_notice(&group, &actor, &event, Some(&long)).expect("notice");
        assert!(!included, "the line waits; the message still goes out");
        assert!(!notice.contains("xxx"));
        assert!(notice.contains("mode=send.\n界"), "{notice}");
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

        let (notice, _) = claude_notice(&group, &actor, &event, None).expect("notice");

        assert!(notice.contains("mode=notify.\nYou are now x."), "{notice}");
    }
}

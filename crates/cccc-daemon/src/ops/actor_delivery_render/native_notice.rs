use cccc_contracts::{Actor, ActorRole, Event};
use cccc_core::GroupDoc;
use serde_json::json;

// Claude Code 2.1.310 exposes only the first 500 raw characters of a Monitor
// stdout record. Budget the complete JSON line in bytes, including escaping,
// leaving room for the newline. The ledger remains the full-content authority.
const MONITOR_LINE_BYTES: usize = 480;

pub(crate) fn claude_notice(group: &GroupDoc, actor: &Actor, event: &Event) -> Option<String> {
    let lookup = json!({"group_id":group.group_id,"event_id":event.id});
    let role = match cccc_core::actors::effective_role(group, &actor.id) {
        Some(ActorRole::Foreman) => "foreman",
        _ => "peer",
    };
    let scope = cccc_core::group_scope::effective_scope(group, Some(actor));
    let scope_key = scope.map_or("none", |scope| scope.scope_key.as_str());
    let mode = event.data["message_mode"].as_str().unwrap_or("notify");
    let mut text = format!(
        "[CCCC] Read full message and instructions: cccc_message_history({lookup}).\nYou are {} ({role}); scope={scope_key}; from={}; mode={mode}.\n",
        actor.id, event.by,
    );
    if encoded_len(&text) > MONITOR_LINE_BYTES {
        return None;
    }
    let header_len = text.len();
    text.extend(
        event.data["text"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(80),
    );
    while encoded_len(&text) > MONITOR_LINE_BYTES && text.len() > header_len {
        text.pop();
    }
    Some(text)
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
            let notice = claude_notice(&group, &actor, &event).expect("notice");
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
}

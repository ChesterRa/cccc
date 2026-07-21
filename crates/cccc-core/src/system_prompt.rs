use cccc_contracts::{Actor, ActorRole};

use crate::GroupDoc;
use crate::actors::effective_role;

#[must_use]
pub fn render(group: &GroupDoc, actor: &Actor) -> String {
    let enabled: Vec<_> = group
        .actors
        .iter()
        .filter(|item| item.enabled)
        .map(|item| item.id.as_str())
        .collect();
    let role = match effective_role(group, &actor.id) {
        Some(ActorRole::Foreman) => "foreman",
        Some(ActorRole::Peer) | None => "peer",
    };
    let runtime = enum_name(actor.runtime);
    let runner = enum_name(actor.runner);
    let mut lines = vec![
        format!(
            "[CCCC] You are {} ({role}) in group '{}'",
            actor.id, group.title
        ),
        format!("group_id: {}", group.group_id),
        format!("runtime: {runtime} ({runner})"),
    ];
    if !group.topic.trim().is_empty() {
        lines.push(format!("topic: {}", group.topic.trim()));
    }
    if enabled.len() <= 1 {
        lines.push("team: solo (you're the only actor)".into());
    } else {
        lines.push(format!(
            "team: {} actors ({})",
            enabled.len(),
            enabled.join(", ")
        ));
    }
    if runner == "headless" {
        lines.push("runner: headless (process transport with structured CCCC state)".into());
    }
    if !group.scopes.is_empty() {
        lines.push(String::new());
        lines.push("scopes (* = active):".into());
        for scope in &group.scopes {
            let label = if scope.label.is_empty() {
                &scope.scope_key
            } else {
                &scope.label
            };
            let active = if scope.scope_key == group.active_scope_key {
                " *"
            } else {
                ""
            };
            lines.push(format!("  {label}: {}{active}", scope.url));
        }
    }
    lines.extend([
        String::new(),
        "---".into(),
        "Working Style:".into(),
        "- Work like a sharp teammate, not a customer-service script.".into(),
        "- Prefer silence over low-signal chatter; report concrete changes and blockers.".into(),
        String::new(),
        "Platform Invariants:".into(),
        "- No fabrication. Verify before claiming done.".into(),
        "- Use cccc_message_reply for replies; use cccc_message_send for new messages.".into(),
        "- Terminal output is not delivered.".into(),
        "- Once scope is approved, finish it end-to-end.".into(),
    ]);
    let has_visible_peer = actor.internal_kind.is_none()
        && crate::actors::visible(group).any(|item| item.enabled && item.id != actor.id);
    if has_visible_peer {
        lines.push(String::new());
        lines.push(crate::peer_insight::TEAM_MODE_SEED.into());
        lines.push(String::new());
        lines.push(crate::peer_insight::PEER_INSIGHT_RUNTIME_HELP.clone());
    }
    lines.join("\n") + "\n"
}

fn enum_name(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::GroupStore;
    use crate::home::HomeLayout;
    use cccc_contracts::Actor;

    #[test]
    fn renders_identity_and_invariants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("test", "migration").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        let prompt = render(&group, &actor);
        assert!(prompt.contains("You are peer1"));
        assert!(prompt.contains("No fabrication"));
        assert!(prompt.contains(&group.group_id));
    }

    #[test]
    fn peer_insight_help_requires_another_visible_actor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("test", "migration").expect("group");
        let actor = Actor::new("foreman");
        group.actors.push(actor.clone());
        assert!(!render(&group, &actor).contains("Peer Insight Contract"));
        group.actors.push(Actor::new("peer1"));
        assert!(render(&group, &actor).contains("Peer Insight Contract"));
    }
}

use cccc_contracts::{Actor, ActorRole};

use crate::actors::{effective_role, visible};
use crate::group_prompts::{DEFAULT_PREAMBLE_BODY, read_preamble};
use crate::group_scope::{EffectiveScope, configured_scope};
use crate::{GroupDoc, GroupStore, HomeLayout};

pub const NEW_MESSAGE_MODE_GUIDANCE: &str = "New messages: use mode=\"mail\" unless delayed awareness would cost more than interrupting the recipient; then use mode=\"send\". Use mode=\"request_reply\" only when a concrete reply is also required. Do not send routine noise. Mail is agent-only. Never mix user and agent recipients in one message; send separate messages when both audiences need different actions.";
pub const NATIVE_MESSAGE_MODE_GUIDANCE: &str = "Native collaboration defaults to mode=\"send\" for timely communication. Use mode=\"request_reply\" when a concrete reply is required; choose mode=\"mail\" explicitly for asynchronous Inbox work, whose notification may be delayed. Do not send routine noise. Mail is agent-only; keep user and agent audiences in separate messages.";
pub const EXISTING_MESSAGE_REPLY_GUIDANCE: &str =
    "For an existing event, use cccc_message_reply instead of cccc_message_send.";

#[must_use]
pub fn render(group: &GroupDoc, actor: &Actor) -> String {
    let scope = configured_scope(group, Some(actor));
    render_with_body(group, actor, DEFAULT_PREAMBLE_BODY, scope.as_ref())
}

#[must_use]
pub fn render_session(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    let scope = configured_scope(group, Some(actor));
    render_session_with_scope(home, group, actor, scope.as_ref())
}

/// The daemon supplies the registered owner's launch workspace, including an
/// explicit unavailable result. Never fall back to saved next-launch settings.
#[must_use]
pub fn render_session_with_scope(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    scope: Option<&EffectiveScope>,
) -> String {
    let custom = GroupStore::new(home.clone()).ok().and_then(|store| {
        read_preamble(&store, &group.group_id)
            .ok()
            .and_then(|prompt| prompt.content)
    });
    render_with_body(
        group,
        actor,
        custom
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_PREAMBLE_BODY),
        scope,
    )
}

fn render_with_body(
    group: &GroupDoc,
    actor: &Actor,
    body: &str,
    scope: Option<&EffectiveScope>,
) -> String {
    let enabled: Vec<_> = visible(group)
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
            actor.id,
            if group.title.is_empty() {
                &group.group_id
            } else {
                &group.title
            }
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
        let shown = enabled.iter().take(8).copied().collect::<Vec<_>>();
        let suffix = if enabled.len() > 8 { "..." } else { "" };
        lines.push(format!(
            "team: {} actors ({}{suffix})",
            enabled.len(),
            shown.join(", ")
        ));
        let foremen = enabled
            .iter()
            .copied()
            .filter(|actor_id| effective_role(group, actor_id) == Some(ActorRole::Foreman))
            .collect::<Vec<_>>();
        if !foremen.is_empty() {
            lines.push(format!("foreman: {}", foremen.join(", ")));
        }
    }
    if !actor.external_session_id.is_empty() {
        lines.push("session: externally owned; CCCC does not host its terminal".into());
    } else if runner == "headless" {
        lines.push("runner: headless (MCP-only, no PTY)".into());
    }
    if let Some(scope) = scope {
        lines.push(format!(
            "effective_scope: {} ({}; CCCC tools)",
            scope.scope_key, scope.source
        ));
        lines.push(format!("workspace: {}", scope.path.display()));
    } else {
        lines.push("effective_scope: unavailable (CCCC tools)".into());
    }
    if !actor.external_session_id.is_empty() {
        lines.push("native process cwd: managed by the original application; CCCC scope selection does not change it".into());
    }
    let scope_lines = group
        .scopes
        .iter()
        .filter(|scope| !scope.url.trim().is_empty())
        .map(|scope| {
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
            format!("  {label}: {}{active}", scope.url)
        })
        .collect::<Vec<_>>();
    if !scope_lines.is_empty() {
        lines.push(String::new());
        lines.push("scopes (* = Group active):".into());
        lines.extend(scope_lines);
    }
    lines.extend([
        String::new(),
        "---".into(),
        "CCCC Protocol:".into(),
        format!("- {}", if actor.external_session_id.is_empty() { NEW_MESSAGE_MODE_GUIDANCE } else { NATIVE_MESSAGE_MODE_GUIDANCE }),
        format!("- {EXISTING_MESSAGE_REPLY_GUIDANCE}"),
        "- Before replying, verify `event_id`; before any message, verify `to` and make the audience explicit when it differs."
            .into(),
        "- Terminal output is not delivered.".into(),
    ]);
    if !actor.external_session_id.is_empty() {
        lines.push(format!("- Pass group_id=\"{}\" to every Group tool. On cold start or context reset, call cccc_bootstrap({{\"group_id\":\"{}\"}}).", group.group_id, group.group_id));
    }
    if enabled.len() > 1 {
        lines.push(
            if role == "foreman" {
                crate::peer_insight::FOREMAN_TEAM_MODE_SEED
            } else {
                crate::peer_insight::TEAM_MODE_SEED
            }
            .into(),
        );
    }
    let header = lines.join("\n").trim_end().to_owned();
    let body = body.trim();
    if body.is_empty() {
        header + "\n"
    } else {
        format!("{header}\n\n{body}\n")
    }
}

fn enum_name(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::{
        EXISTING_MESSAGE_REPLY_GUIDANCE, NATIVE_MESSAGE_MODE_GUIDANCE, NEW_MESSAGE_MODE_GUIDANCE,
        render, render_session,
    };
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
        assert!(prompt.contains(&group.group_id));
        assert!(prompt.contains("use MCP tool `cccc_bootstrap`"));
        assert_eq!(prompt.matches(NEW_MESSAGE_MODE_GUIDANCE).count(), 1);
        assert_eq!(prompt.matches(EXISTING_MESSAGE_REPLY_GUIDANCE).count(), 1);
        assert!(prompt.contains("Before replying, verify `event_id`"));
        assert!(prompt.contains("Terminal output is not delivered."));
        assert!(!prompt.contains("Current Context Snapshot"));
    }

    #[test]
    fn renders_actor_effective_scope_without_relabeling_group_active() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = GroupStore::new(HomeLayout::from_path(temp.path().join("home")).expect("home"))
            .expect("store");
        let mut group = store.create("two scopes", "").expect("group");
        for name in ["A", "B"] {
            group.scopes.push(crate::Scope {
                scope_key: format!("scope_{name}"),
                url: temp.path().join(name).to_string_lossy().into_owned(),
                label: name.into(),
                git_remote: String::new(),
            });
        }
        let mut actor = Actor::new("native-peer");
        actor.default_scope_key = "scope_B".into();
        actor.external_session_id = "codex:fixture".into();
        group.actors.push(actor.clone());
        for active in ["scope_A", "scope_B", "scope_A"] {
            group.active_scope_key = active.into();
            let fixed = render(&group, &actor);
            assert!(
                fixed.contains("effective_scope: scope_B (actor_default; CCCC tools)"),
                "{fixed}"
            );
            assert!(fixed.contains(&format!("workspace: {}", group.scopes[1].url)));
            assert!(fixed.contains("scopes (* = Group active):"));
            assert!(fixed.contains("native process cwd: managed by the original application"));
            let index = usize::from(active == "scope_B");
            assert!(fixed.contains(&format!(
                "  {}: {} *",
                group.scopes[index].label, group.scopes[index].url
            )));

            let mut following = actor.clone();
            following.default_scope_key.clear();
            let prompt = render(&group, &following);
            assert!(
                prompt.contains(&format!(
                    "effective_scope: {active} (group_active; CCCC tools)"
                )),
                "{prompt}"
            );
            assert!(prompt.contains(&format!("workspace: {}", group.scopes[index].url)));
        }
    }

    #[test]
    fn session_prompt_uses_python_compatible_group_override_without_context_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("test", "migration").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        let prompt_dir = store
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("prompts");
        std::fs::create_dir_all(&prompt_dir).expect("prompt dir");
        std::fs::write(prompt_dir.join("CCCC_PREAMBLE.md"), "custom startup")
            .expect("custom preamble");

        let prompt = render_session(&home, &group, &actor);
        assert!(prompt.ends_with("custom startup\n"));
        assert!(!prompt.contains("cccc_bootstrap"));
        assert!(!prompt.contains("Current Context Snapshot"));
    }

    #[test]
    fn peer_insight_help_requires_another_visible_actor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("test", "migration").expect("group");
        let actor = Actor::new("foreman");
        group.actors.push(actor.clone());
        assert!(!render(&group, &actor).contains(crate::peer_insight::FOREMAN_TEAM_MODE_SEED));
        group.actors.push(Actor::new("peer1"));
        assert!(render(&group, &actor).contains(crate::peer_insight::FOREMAN_TEAM_MODE_SEED));
        assert!(!render(&group, &actor).contains(crate::peer_insight::TEAM_MODE_SEED));

        let peer = group.actors.last().expect("peer");
        assert!(render(&group, peer).contains(crate::peer_insight::TEAM_MODE_SEED));
        assert!(!render(&group, peer).contains(crate::peer_insight::FOREMAN_TEAM_MODE_SEED));
    }

    #[test]
    fn native_prompt_defaults_to_send_and_requires_the_explicit_group_with_custom_body_preserved() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Native collaboration", "").expect("group");
        let mut actor = Actor::new("native");
        actor.external_session_id = "codex:thread".into();
        group.actors.push(actor.clone());
        let prompt_dir = store
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("prompts");
        std::fs::create_dir_all(&prompt_dir).expect("prompts");
        std::fs::write(prompt_dir.join("CCCC_PREAMBLE.md"), "custom startup").expect("custom body");
        let prompt = render_session(&home, &group, &actor);
        assert_eq!(prompt.matches(NATIVE_MESSAGE_MODE_GUIDANCE).count(), 1);
        assert!(!prompt.contains(NEW_MESSAGE_MODE_GUIDANCE));
        assert!(prompt.contains(&format!(
            "cccc_bootstrap({{\"group_id\":\"{}\"}})",
            group.group_id
        )));
        assert!(prompt.contains("session: externally owned"));
        assert!(!prompt.contains("MCP-only, no PTY"));
        assert!(prompt.ends_with("custom startup\n"));
    }
}

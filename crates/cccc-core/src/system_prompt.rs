use cccc_contracts::{Actor, ActorRole};
use std::path::Path;

use crate::actors::{effective_role, visible};
use crate::group_prompts::{DEFAULT_PREAMBLE_BODY, read_preamble};
use crate::{GroupDoc, GroupStore, HomeLayout};

mod voice_secretary;

pub const MESSAGE_DELIVERY_GUIDANCE: &str = "New messages: use mode=\"mail\" unless delayed awareness would cost more than interrupting the recipient; then use mode=\"send\". Use mode=\"request_reply\" only when a concrete reply is also required. Do not send routine noise; use cccc_message_reply for an existing event. Mail is agent-only. Never mix user and agent recipients in one message; send separate messages when both audiences need different actions.";

#[must_use]
pub fn render(group: &GroupDoc, actor: &Actor) -> String {
    if voice_secretary::is_actor(actor) {
        return voice_secretary::render(group, actor);
    }
    render_with_body(group, actor, DEFAULT_PREAMBLE_BODY)
}

#[must_use]
pub fn render_session(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> String {
    if voice_secretary::is_actor(actor) {
        return voice_secretary::render(group, actor);
    }
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
    )
}

fn render_with_body(group: &GroupDoc, actor: &Actor, body: &str) -> String {
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
    if runner == "headless" {
        lines.push("runner: headless (MCP-only, no PTY)".into());
    }
    lines.push(project_line(group));
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
        lines.push("scopes (* = active):".into());
        lines.extend(scope_lines);
    }
    lines.extend([
        String::new(),
        "---".into(),
        "CCCC Protocol:".into(),
        format!("- {MESSAGE_DELIVERY_GUIDANCE}"),
        "- Before sending, verify `reply_to` and `to`; make the audience explicit when it differs."
            .into(),
        "- Terminal output is not delivered.".into(),
    ]);
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

fn project_line(group: &GroupDoc) -> String {
    let root = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key && !scope.url.trim().is_empty())
        .or_else(|| {
            group
                .scopes
                .iter()
                .find(|scope| !scope.url.trim().is_empty())
        })
        .map(|scope| expanded_path(&scope.url));
    let Some(root) = root else {
        return "project: PROJECT.md missing (no scope attached)".into();
    };
    let upper = root.join("PROJECT.md");
    let lower = root.join("project.md");
    if upper.exists() {
        format!("project: PROJECT.md found ({})", upper.display())
    } else if lower.exists() {
        format!("project: PROJECT.md found ({})", lower.display())
    } else {
        format!(
            "project: PROJECT.md missing (expected at {})",
            upper.display()
        )
    }
}

fn expanded_path(value: &str) -> std::path::PathBuf {
    let Some(suffix) = value.strip_prefix("~/") else {
        return Path::new(value).to_path_buf();
    };
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .map_or_else(|| Path::new(value).to_path_buf(), |home| home.join(suffix))
}

fn enum_name(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::{MESSAGE_DELIVERY_GUIDANCE, render, render_session};
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
        assert_eq!(prompt.matches(MESSAGE_DELIVERY_GUIDANCE).count(), 1);
        assert!(prompt.contains("verify `reply_to` and `to`"));
        assert!(prompt.contains("Terminal output is not delivered."));
        assert!(!prompt.contains("Current Context Snapshot"));
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
    fn voice_secretary_uses_its_python_compatible_runtime_prompt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("test", "migration").expect("group");
        let mut actor = Actor::new("voice-secretary");
        actor.internal_kind = Some("voice_secretary".into());
        group.actors.push(actor.clone());

        let prompt = render(&group, &actor);
        assert!(prompt.starts_with("[CCCC Voice Secretary Runtime Actor]\n"));
        assert!(prompt.contains("The input_envelope is the canonical work item."));
        assert!(!prompt.contains("CCCC Protocol:"));
    }
}

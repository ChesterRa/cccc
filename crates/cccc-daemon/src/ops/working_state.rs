use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

pub fn runtime_actor_fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
) -> Map<String, Value> {
    let runner_effective = if super::actor_runtime::is_structured(actor) {
        "headless"
    } else {
        "pty"
    };
    fields(home, actor, group_id, running, runner_effective)
}

fn fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
    runner_effective: &str,
) -> Map<String, Value> {
    let local_state = (running && super::local_headless::supports(actor))
        .then(|| super::local_headless::status(group_id, &actor.id))
        .flatten();
    let hook_runtime = match actor.runtime {
        ActorRuntime::Codex => Some("codex"),
        ActorRuntime::Claude => Some("claude"),
        _ => None,
    };
    let hook_state = (running && !super::local_headless::supports(actor))
        .then(|| {
            hook_runtime.and_then(|runtime| {
                cccc_core::codex_hook_state::read_runtime(home, runtime, group_id, &actor.id)
            })
        })
        .flatten();
    let (state, reason, updated_at, active_task_id) = if !running {
        (
            "stopped".to_owned(),
            "runner_not_running".to_owned(),
            None,
            None,
        )
    } else if let Some(local_state) = local_state {
        (
            local_state.status,
            "provider_headless_session".to_owned(),
            Some(local_state.updated_at),
            local_state.task_id,
        )
    } else if let Some(hook_state) = hook_state {
        let reason = if hook_state.v == 2 {
            format!(
                "{}_hook_legacy_unfenced_{}",
                hook_state.runtime, hook_state.event
            )
        } else if hook_state.observation == "pty_fail_closed" {
            format!("claude_pty_fail_closed_{}", hook_state.event)
        } else {
            format!("{}_hook_{}", hook_state.runtime, hook_state.event)
        };
        (
            hook_state.status,
            reason,
            Some(hook_state.updated_at),
            hook_state.turn_id,
        )
    } else if hook_runtime.is_some() && runner_effective == "pty" {
        (
            "waiting".to_owned(),
            format!("{}_hook_pending", hook_runtime.unwrap_or_default()),
            None,
            None,
        )
    } else if runner_effective == "headless" {
        ("idle".to_owned(), "headless_running".to_owned(), None, None)
    } else {
        (
            "waiting".to_owned(),
            "pty_running_state_unknown".to_owned(),
            None,
            None,
        )
    };

    Map::from_iter([
        ("idle_seconds".into(), Value::Null),
        ("runner_effective".into(), json!(runner_effective)),
        ("effective_working_state".into(), json!(state)),
        ("effective_working_reason".into(), json!(reason)),
        ("effective_working_updated_at".into(), json!(updated_at)),
        ("effective_active_task_id".into(), json!(active_task_id)),
    ])
}

#[cfg(test)]
mod tests {
    use super::fields;
    use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
    use cccc_core::HomeLayout;
    use serde_json::json;

    #[test]
    fn codex_state_comes_from_hooks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Codex;
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "codex",
            "g_test",
            "peer1",
            "token",
            "HookPending",
        )
        .expect("launch");
        cccc_core::codex_hook_state::record(
            &home,
            "g_test",
            "peer1",
            "token",
            &json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        )
        .expect("session state");
        cccc_core::codex_hook_state::record(
            &home,
            "g_test",
            "peer1",
            "token",
            &json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"s1",
                "turn_id":"turn-1"
            }),
        )
        .expect("hook state");

        let state = fields(&home, &actor, "g_test", true, "pty");
        assert_eq!(state["effective_working_state"], "working");
        assert_eq!(
            state["effective_working_reason"],
            "codex_hook_UserPromptSubmit"
        );
        assert_eq!(state["effective_active_task_id"], "turn-1");
    }

    #[test]
    fn externally_managed_headless_actor_without_local_session_is_idle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Custom;
        actor.runner = RunnerKind::Headless;

        let state = fields(&home, &actor, "g_test", true, "headless");
        assert_eq!(state["effective_working_state"], "idle");
        assert_eq!(state["effective_working_reason"], "headless_running");
    }

    #[test]
    fn claude_pty_state_comes_from_hooks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Claude;
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "claude",
            "g_test",
            "peer1",
            "token",
            "HookPending",
        )
        .expect("launch");
        cccc_core::codex_hook_state::record_runtime(
            &home,
            "claude",
            "g_test",
            "peer1",
            "token",
            &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
        )
        .expect("hook state");

        let state = fields(&home, &actor, "g_test", true, "pty");
        assert_eq!(state["effective_working_state"], "idle");
        assert_eq!(
            state["effective_working_reason"],
            "claude_pty_fail_closed_SessionStart"
        );
    }

    #[test]
    fn claude_pty_without_hook_is_pending_not_terminal_inferred() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Claude;

        let state = fields(&home, &actor, "g_test", true, "pty");
        assert_eq!(state["effective_working_state"], "waiting");
        assert_eq!(state["effective_working_reason"], "claude_hook_pending");
    }

    #[test]
    fn claude_hook_setup_issue_is_visible() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Claude;
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "claude",
            "g_test",
            "peer1",
            "token",
            "HookUnavailableVersion",
        )
        .expect("setup issue");

        let state = fields(&home, &actor, "g_test", true, "pty");
        assert_eq!(state["effective_working_state"], "waiting");
        assert_eq!(
            state["effective_working_reason"],
            "claude_pty_fail_closed_HookUnavailableVersion"
        );
    }
}

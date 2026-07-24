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
    let hook_state = (running
        && actor.runtime == ActorRuntime::Codex
        && !super::local_headless::supports(actor))
    .then(|| cccc_core::codex_hook_state::read(home, group_id, &actor.id))
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
        let reason = format!("codex_hook_{}", hook_state.event);
        (
            hook_state.status,
            reason,
            Some(hook_state.updated_at),
            hook_state.turn_id,
        )
    } else if actor.runtime == ActorRuntime::Codex && runner_effective == "pty" {
        (
            "waiting".to_owned(),
            "codex_hook_pending".to_owned(),
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
        cccc_core::codex_hook_state::record(
            &home,
            "g_test",
            "peer1",
            &json!({"hook_event_name":"UserPromptSubmit","turn_id":"turn-1"}),
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
}

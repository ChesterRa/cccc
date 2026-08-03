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
    let pid = running
        .then(|| super::actor_runtime::status(group_id, &actor.id))
        .flatten()
        .and_then(|status| status.pid);
    fields(home, actor, group_id, running, runner_effective, pid)
}

pub(super) fn fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
    runner_effective: &str,
    pid: Option<u32>,
) -> Map<String, Value> {
    let local_state = (running && super::local_headless::supports(actor))
        .then(|| super::local_headless::status(group_id, &actor.id))
        .flatten();
    let hook_runtime = match actor.runtime {
        ActorRuntime::Codex => Some("codex"),
        ActorRuntime::Claude => Some("claude"),
        _ => None,
    };
    let capability = hook_runtime.and_then(|runtime| {
        super::runtime_hook_session::validated(home, runtime, group_id, &actor.id, pid)
    });
    let hook_state = (running && !super::local_headless::supports(actor))
        .then(|| {
            hook_runtime.and_then(|runtime| {
                cccc_core::codex_hook_state::read_runtime(home, runtime, group_id, &actor.id)
                    .filter(|state| {
                        capability
                            .as_ref()
                            .is_some_and(|current| current.launch_token == state.launch_token)
                    })
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

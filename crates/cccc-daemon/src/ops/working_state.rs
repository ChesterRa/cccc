use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

pub fn runtime_actor_fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
) -> Map<String, Value> {
    let actual = super::local_headless::structured_state(group_id, &actor.id);
    let retained_pty = cccc_runtime::status(group_id, &actor.id).is_ok_and(|status| status.running);
    let runner_effective = if let Some(actual) = actual.as_ref() {
        actual["runner"].as_str().unwrap_or("pty")
    } else if retained_pty {
        "pty"
    } else {
        match actor.runtime.runner_for(actor.runtime_mode) {
            RunnerKind::Headless => "headless",
            RunnerKind::Pty => "pty",
        }
    };
    fields(home, actor, group_id, running, runner_effective)
}

pub(super) fn fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
    runner_effective: &str,
) -> Map<String, Value> {
    let managed_session = super::local_headless::running(group_id, &actor.id)
        || super::local_headless::uses_managed_session(actor);
    let local_state = running
        .then(|| super::local_headless::status(group_id, &actor.id))
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
            local_state.reason.unwrap_or_else(|| {
                if runner_effective == "pty" {
                    "managed_agent_session".to_owned()
                } else {
                    "provider_headless_session".to_owned()
                }
            }),
            Some(local_state.updated_at),
            local_state.task_id,
        )
    } else if managed_session {
        (
            "waiting".to_owned(),
            "managed_agent_session_pending".to_owned(),
            None,
            None,
        )
    } else if actor.runtime == ActorRuntime::ChatgptDots {
        match super::runtime_state::actor_state(home, group_id, &actor.id) {
            Ok(runtime) => (
                if runtime["active_turn_id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty())
                {
                    "working"
                } else {
                    "waiting"
                }
                .to_owned(),
                "dots_event_turn".to_owned(),
                runtime["updated_at"].as_str().map(str::to_owned),
                runtime["active_turn_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned),
            ),
            Err(_) => (
                "waiting".to_owned(),
                "dots_state_unavailable".to_owned(),
                None,
                None,
            ),
        }
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

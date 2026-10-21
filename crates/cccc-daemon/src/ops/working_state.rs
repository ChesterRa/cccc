use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::time::Duration;

/// A terminal (PTY) Actor without a managed session reports no turns. The
/// daemon reads every byte the process writes to its terminal, so output this
/// recent is the runtime-independent evidence that it is working.
/// `CCCC_PTY_ACTIVE_WINDOW_SECONDS` overrides it (1..=600).
const DEFAULT_PTY_ACTIVE_WINDOW: Duration = Duration::from_secs(10);

fn pty_active_window() -> Duration {
    std::env::var("CCCC_PTY_ACTIVE_WINDOW_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(DEFAULT_PTY_ACTIVE_WINDOW, |seconds| {
            Duration::from_secs(seconds.clamp(1, 600))
        })
}

pub(super) struct PtyOutputActivity {
    pub state: &'static str,
    pub reason: &'static str,
    pub last_output_at: Option<String>,
    pub idle_seconds: Option<u64>,
}

pub(super) fn pty_output_activity(
    group_id: &str,
    actor_id: &str,
    window: Duration,
) -> PtyOutputActivity {
    let Some(quiet) = cccc_runtime::since_last_output(group_id, actor_id)
        .ok()
        .flatten()
    else {
        return PtyOutputActivity {
            state: "waiting",
            reason: "pty_running_state_unknown",
            last_output_at: None,
            idle_seconds: None,
        };
    };
    let (state, reason) = if quiet <= window {
        ("working", "pty_output_recent")
    } else {
        ("idle", "pty_output_quiet")
    };
    PtyOutputActivity {
        state,
        reason,
        last_output_at: chrono::Duration::from_std(quiet).ok().map(|quiet| {
            (chrono::Utc::now() - quiet).to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        }),
        idle_seconds: Some(quiet.as_secs()),
    }
}

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
    let mut idle_seconds = None;
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
        let activity = pty_output_activity(group_id, &actor.id, pty_active_window());
        idle_seconds = activity.idle_seconds;
        (
            activity.state.to_owned(),
            activity.reason.to_owned(),
            activity.last_output_at,
            None,
        )
    };

    Map::from_iter([
        ("idle_seconds".into(), json!(idle_seconds)),
        ("runner_effective".into(), json!(runner_effective)),
        ("effective_working_state".into(), json!(state)),
        ("effective_working_reason".into(), json!(reason)),
        ("effective_working_updated_at".into(), json!(updated_at)),
        ("effective_active_task_id".into(), json!(active_task_id)),
    ])
}

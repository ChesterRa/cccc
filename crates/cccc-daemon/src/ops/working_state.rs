use crate::session_owner::runtime as cccc_runtime;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

/// When a live terminal session last wrote output, for display only. Output
/// does not establish whether the process is working: a model can compute
/// silently, and a spinner or a waiting prompt can keep printing. The working
/// state therefore never reads these values.
fn pty_output_observation(group_id: &str, actor_id: &str) -> (Value, Value) {
    let Ok(Some(observed)) = cccc_runtime::last_output(group_id, actor_id) else {
        return (Value::Null, Value::Null);
    };
    let last_output_at = chrono::DateTime::<chrono::Utc>::from(observed.last_output_at)
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let quiet_seconds = observed.quiet_for.as_secs();
    (Value::String(last_output_at), json!(quiet_seconds))
}

fn live_antigravity_terminal(
    group_id: &str,
    actor_id: &str,
) -> Option<cccc_runtime::SessionStatus> {
    let status = cccc_runtime::status(group_id, actor_id).ok()?;
    if !status.running {
        return None;
    }
    #[cfg(unix)]
    let launched_runtime = crate::session_owner::snapshot(group_id, actor_id)
        .ok()
        .flatten()
        .and_then(|snapshot| snapshot.terminal_launch)
        .or(status.launch)
        .map(|launch| launch.runtime);
    #[cfg(not(unix))]
    let launched_runtime = status.launch.map(|launch| launch.runtime);
    if launched_runtime.is_some_and(|runtime| runtime != ActorRuntime::Antigravity) {
        return None;
    }
    Some(status)
}

pub(crate) fn antigravity_hook_idle_for_status(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    status: &cccc_runtime::SessionStatus,
) -> bool {
    status.running
        && cccc_core::antigravity_hook::read_live_state(
            home,
            group_id,
            actor_id,
            status.pid,
            &status.started_at,
        )
        .is_some_and(|state| state.status == "idle")
}

/// True when a running Actor reports idle state through either a managed
/// session or Antigravity's native lifecycle hooks, admitting early Mail
/// notices without interrupting active work.
pub(crate) fn ready_for_mail_notice(home: &HomeLayout, group_id: &str, actor_id: &str) -> bool {
    if super::local_headless::ready_for_mail_notice(group_id, actor_id) {
        return true;
    }
    live_antigravity_terminal(group_id, actor_id)
        .is_some_and(|status| antigravity_hook_idle_for_status(home, group_id, actor_id, &status))
}

fn live_antigravity_hook_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    configured_runtime: ActorRuntime,
) -> Option<cccc_core::antigravity_hook::HookState> {
    let status = live_antigravity_terminal(group_id, actor_id)?;
    #[cfg(unix)]
    let launched_runtime = crate::session_owner::snapshot(group_id, actor_id)
        .ok()
        .flatten()
        .and_then(|snapshot| snapshot.terminal_launch)
        .or(status.launch)
        .map(|launch| launch.runtime);
    #[cfg(not(unix))]
    let launched_runtime = status.launch.map(|launch| launch.runtime);
    if configured_runtime != ActorRuntime::Antigravity
        && launched_runtime != Some(ActorRuntime::Antigravity)
    {
        return None;
    }
    cccc_core::antigravity_hook::read_live_state(
        home,
        group_id,
        actor_id,
        status.pid,
        &status.started_at,
    )
}

pub fn runtime_actor_fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
) -> Map<String, Value> {
    crate::session_owner::read_scope(|| runtime_actor_snapshot(home, actor, group_id, running))
}

fn runtime_actor_snapshot(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
) -> Map<String, Value> {
    if actor.external_session_id.is_empty()
        && let Err(error) =
            crate::session_owner::require_available(home, Some(group_id), Some(&actor.id))
    {
        return json!({
            "effective_working_state":"unknown",
            "effective_working_reason":"runtime_owner_unavailable",
            "effective_working_updated_at":null,
            "effective_active_task_id":null,
            "runtime_observation_error":error.to_string()
        })
        .as_object()
        .cloned()
        .expect("working state projection");
    }
    let actual = super::local_headless::structured_state(group_id, &actor.id);
    let retained_pty = cccc_runtime::status(group_id, &actor.id).is_ok_and(|status| status.running);
    let runner_effective = if !actor.external_session_id.is_empty() {
        "headless"
    } else if let Some(actual) = actual.as_ref() {
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
    let live_managed = super::local_headless::running(group_id, &actor.id);
    let configured_managed = super::local_headless::uses_managed_session(actor);
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
    } else if live_managed {
        (
            "waiting".to_owned(),
            "managed_agent_session_pending".to_owned(),
            None,
            None,
        )
    } else if runner_effective == "pty"
        && let Some(hook) = live_antigravity_hook_state(home, group_id, &actor.id, actor.runtime)
    {
        (
            hook.status,
            "antigravity_hook".to_owned(),
            Some(hook.updated_at),
            None,
        )
    } else if configured_managed {
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
        // A running process alone proves neither an idle turn nor its age.
        (
            "unknown".to_owned(),
            "headless_running_state_unknown".to_owned(),
            None,
            None,
        )
    } else {
        (
            "waiting".to_owned(),
            "pty_running_state_unknown".to_owned(),
            None,
            None,
        )
    };
    let (pty_last_output_at, pty_quiet_seconds) = if running && runner_effective == "pty" {
        pty_output_observation(group_id, &actor.id)
    } else {
        (Value::Null, Value::Null)
    };

    Map::from_iter([
        ("idle_seconds".into(), Value::Null),
        ("runner_effective".into(), json!(runner_effective)),
        ("effective_working_state".into(), json!(state)),
        ("effective_working_reason".into(), json!(reason)),
        ("effective_working_updated_at".into(), json!(updated_at)),
        ("effective_active_task_id".into(), json!(active_task_id)),
        ("pty_last_output_at".into(), pty_last_output_at),
        ("pty_quiet_seconds".into(), pty_quiet_seconds),
    ])
}

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

pub fn runtime_actor_fields(
    home: &HomeLayout,
    actor: &Actor,
    group_id: &str,
    running: bool,
) -> Map<String, Value> {
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
        // The runner's own turn record. `status` is authoritative because a
        // stopped or waiting runner can leave a stale turn id behind. A live
        // turn id is still a working fallback for records written before the
        // status field existed. No runner lock is taken and the ledger is
        // never rescanned — `actor_state` reads the small per-actor record the
        // runner updates on its turn events. No record is not "idle": the
        // state cannot be known, so the reason says so.
        match super::runtime_state::actor_state(home, group_id, &actor.id) {
            Ok(runtime) if !runtime.is_null() => {
                let status = runtime["status"].as_str().filter(|value| !value.is_empty());
                let active_turn = runtime["active_turn_id"]
                    .as_str()
                    .filter(|value| !value.is_empty());
                let last_turn_end = runtime["last_turn_end"]
                    .as_str()
                    .filter(|value| !value.is_empty());
                // Idle is evidence-backed: an idle status without a turn end
                // cannot say when the runner last stopped working, so report
                // the unknown state instead. A live turn id is only the
                // legacy fallback for records written before `status` existed;
                // an unrecognized status is not treated as working.
                let (state, reason) = match status {
                    Some("working") => ("working", "headless_turn_record"),
                    Some("idle") if last_turn_end.is_some() => ("idle", "headless_turn_record"),
                    Some("waiting") => ("waiting", "headless_turn_record"),
                    Some("stopped") => ("stopped", "headless_turn_record"),
                    None if active_turn.is_some() => ("working", "headless_turn_record"),
                    _ => ("waiting", "headless_state_unknown"),
                };
                let updated_at = if state == "working" {
                    runtime["updated_at"].as_str()
                } else {
                    last_turn_end
                }
                .map(str::to_owned);
                let active_task_id = if state == "working" {
                    runtime["task_id"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                } else {
                    None
                };
                (
                    state.to_owned(),
                    reason.to_owned(),
                    updated_at,
                    active_task_id,
                )
            }
            _ => (
                "waiting".to_owned(),
                "headless_state_unknown".to_owned(),
                None,
                None,
            ),
        }
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

use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::SessionStatus;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{hook_launch, runtime_session, schedule_capture};

#[path = "resume_verification_registry.rs"]
mod registry;

const CAPTURE_DELAY: Duration = Duration::from_secs(2);
const FAILURE_MONITOR_DURATION: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(super) fn cancel(group_id: &str, actor_id: &str) {
    registry::cancel(group_id, actor_id);
}

pub(super) fn cancel_if_current(group_id: &str, actor_id: &str, started_at: &str) {
    registry::cancel_if_current(group_id, actor_id, started_at);
}

pub(super) fn cancel_all() {
    registry::cancel_all();
}

pub(super) fn is_monitoring(status: &SessionStatus) -> bool {
    registry::is_current(&status.group_id, &status.actor_id, &status.started_at)
}

#[cfg(test)]
pub(super) fn register_for_test(status: &SessionStatus) -> impl Drop {
    registry::Registration::new(&status.group_id, &status.actor_id, &status.started_at)
}

#[derive(Clone, Copy)]
struct VerificationTiming {
    capture_delay: Duration,
    monitor_duration: Duration,
    poll_interval: Duration,
}

impl Default for VerificationTiming {
    fn default() -> Self {
        Self {
            capture_delay: CAPTURE_DELAY,
            monitor_duration: FAILURE_MONITOR_DURATION,
            poll_interval: POLL_INTERVAL,
        }
    }
}

pub(super) fn schedule(
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    base_command: Vec<String>,
    resumed_status: SessionStatus,
) {
    schedule_with_timing(
        home,
        group,
        actor,
        cwd,
        env,
        base_command,
        resumed_status,
        VerificationTiming::default(),
    );
}

#[allow(clippy::too_many_arguments)]
fn schedule_with_timing(
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    base_command: Vec<String>,
    resumed_status: SessionStatus,
    timing: VerificationTiming,
) {
    let registration =
        registry::Registration::new(&group.group_id, &actor.id, &resumed_status.started_at);
    let registration_key = (group.group_id.clone(), actor.id.clone());
    let spawn = std::thread::Builder::new()
        .name(format!(
            "cccc-resume-verify:{}:{}",
            group.group_id, actor.id
        ))
        .spawn(move || {
            let _registration = registration;
            let started = Instant::now();
            let capture_at = started + timing.capture_delay;
            let deadline = started + timing.monitor_duration.max(timing.capture_delay);
            let mut capture_scheduled = false;
            let error = loop {
                if !registry::is_current(&group.group_id, &actor.id, &resumed_status.started_at) {
                    return;
                }
                let current = match cccc_runtime::status(&group.group_id, &actor.id) {
                    Ok(current) => current,
                    Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {
                        break Some("provider resume process disappeared early".to_owned());
                    }
                    Err(error) => {
                        tracing::warn!(
                            group_id = %group.group_id,
                            actor_id = %actor.id,
                            %error,
                            "failed to inspect resumed actor"
                        );
                        return;
                    }
                };
                if current.started_at != resumed_status.started_at {
                    return;
                }
                if !current.running {
                    break Some("provider resume process exited early".to_owned());
                }
                if let Some(message) = runtime_session::resume_failure(&group.group_id, &actor.id) {
                    break Some(message);
                }
                let now = Instant::now();
                if !capture_scheduled && now >= capture_at {
                    schedule_capture(
                        &home,
                        &group,
                        &actor,
                        cwd.clone(),
                        base_command.clone(),
                        &resumed_status,
                    );
                    capture_scheduled = true;
                }
                if now >= deadline {
                    break None;
                }
                std::thread::sleep(timing.poll_interval);
            };

            let Some(error) = error else {
                if !capture_scheduled {
                    schedule_capture(&home, &group, &actor, cwd, base_command, &resumed_status);
                }
                return;
            };
            let fallback = super::super::runtime_hook_session::with_launch_lock(
                &group.group_id,
                &actor.id,
                || {
                    let start_permit = match crate::runtime_start_gate::permit(&home) {
                        Ok(permit) => permit,
                        Err(_) => return None,
                    };
                    if !registry::is_current(&group.group_id, &actor.id, &resumed_status.started_at)
                    {
                        return None;
                    }
                    let stopped = match cccc_runtime::stop_if_started_at(
                        &group.group_id,
                        &actor.id,
                        &resumed_status.started_at,
                    ) {
                        Ok(stopped) => stopped,
                        Err(stop_error) => {
                            tracing::warn!(
                                group_id = %group.group_id,
                                actor_id = %actor.id,
                                %stop_error,
                                "failed to stop rejected resumed actor"
                            );
                            return None;
                        }
                    };
                    if stopped.is_none() {
                        match cccc_runtime::status(&group.group_id, &actor.id) {
                            Ok(current)
                                if current.running
                                    || current.started_at != resumed_status.started_at =>
                            {
                                return None;
                            }
                            Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {}
                            Err(status_error) => {
                                tracing::warn!(
                                    group_id = %group.group_id,
                                    actor_id = %actor.id,
                                    %status_error,
                                    "failed to verify rejected resumed actor ownership"
                                );
                                return None;
                            }
                        }
                    }
                    super::super::runtime_hook_session::revoke(&group.group_id, &actor.id);
                    if let Err(persist_error) = runtime_session::mark_resume_failed(
                        &home,
                        &group.group_id,
                        &actor.id,
                        &error,
                    ) {
                        tracing::warn!(
                            %persist_error,
                            group_id = %group.group_id,
                            actor_id = %actor.id,
                            "failed to persist resume failure"
                        );
                    }
                    let fresh_command = if actor.runtime == ActorRuntime::Grok {
                        runtime_session::prepare_fresh_grok_command(
                            &home,
                            &group.group_id,
                            &actor.id,
                            &cwd,
                            &base_command,
                        )
                        .command
                    } else {
                        base_command.clone()
                    };
                    Some(hook_launch::launch_serialized_with_permit(
                        &start_permit,
                        &home,
                        &group,
                        &actor,
                        &cwd,
                        &env,
                        fresh_command,
                    ))
                },
            );
            match fallback {
                None => {}
                Some(Ok(fresh)) => {
                    schedule_capture(&home, &group, &actor, cwd, base_command, &fresh);
                }
                Some(Err(fallback_error)) => tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %fallback_error.message,
                    "failed to start fresh actor after resume failure"
                ),
            }
        });
    if spawn.is_err() {
        cancel(&registration_key.0, &registration_key.1);
    }
}

#[cfg(all(test, unix))]
#[path = "resume_verification_tests.rs"]
mod tests;

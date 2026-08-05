use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::SessionStatus;
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{hook_launch, runtime_session, schedule_capture};

pub(super) fn schedule(
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    base_command: Vec<String>,
    resumed_status: SessionStatus,
) {
    let _ = std::thread::Builder::new()
        .name(format!(
            "cccc-resume-verify:{}:{}",
            group.group_id, actor.id
        ))
        .spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut error = None;
            while std::time::Instant::now() < deadline {
                let Ok(current) = cccc_runtime::status(&group.group_id, &actor.id) else {
                    return;
                };
                if current.started_at != resumed_status.started_at {
                    return;
                }
                if !current.running {
                    error = Some("provider resume process exited early".to_owned());
                    break;
                }
                if let Some(message) = runtime_session::resume_failure(&group.group_id, &actor.id) {
                    error = Some(message);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            let Some(error) = error else {
                schedule_capture(&home, &group, &actor, cwd, base_command, &resumed_status);
                return;
            };
            let stopped = cccc_runtime::stop_if_started_at(
                &group.group_id,
                &actor.id,
                &resumed_status.started_at,
            );
            if !matches!(stopped, Ok(Some(_))) {
                return;
            }
            super::super::runtime_hook_session::revoke(&group.group_id, &actor.id);
            if let Err(persist_error) =
                runtime_session::mark_resume_failed(&home, &group.group_id, &actor.id, &error)
            {
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
            match hook_launch::launch(&home, &group, &actor, &cwd, &env, fresh_command) {
                Ok(fresh) => {
                    schedule_capture(&home, &group, &actor, cwd, base_command, &fresh);
                }
                Err(fallback_error) => tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %fallback_error.message,
                    "failed to start fresh actor after resume failure"
                ),
            }
        });
}

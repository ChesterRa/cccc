use cccc_contracts::{Actor, ActorRuntime, RuntimeStateSource};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::{LaunchSpec, SessionStatus};
use std::collections::BTreeMap;
use std::path::Path;

use crate::dispatch::OpError;
use crate::ops::{codex_mcp, runtime_hook_session};

#[cfg(test)]
pub(super) fn launch(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    command: Vec<String>,
) -> Result<SessionStatus, OpError> {
    runtime_hook_session::with_launch_lock(&group.group_id, &actor.id, || {
        launch_serialized(home, group, actor, cwd, env, command)
    })
}

pub(super) fn launch_serialized(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    command: Vec<String>,
) -> Result<SessionStatus, OpError> {
    if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
        && status.running
    {
        return Ok(status);
    }
    let start_permit = crate::runtime_start_gate::permit(home)
        .map_err(|message| OpError::new("runtime_shutting_down", message))?;
    launch_serialized_with_permit(&start_permit, home, group, actor, cwd, env, command)
}

pub(super) fn launch_serialized_with_permit(
    _start_permit: &crate::runtime_start_gate::StartPermit,
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    mut command: Vec<String>,
) -> Result<SessionStatus, OpError> {
    let original_command = command.clone();
    let mut launch_env = env.clone();
    let original_env = launch_env.clone();
    codex_mcp::configure_actor_cli(&mut launch_env);

    let hook_eligible = actor.runtime_state_source == RuntimeStateSource::Terminal;
    let setup = match actor.runtime {
        ActorRuntime::Codex if hook_eligible => codex_mcp::configure(
            home,
            &group.group_id,
            &actor.id,
            &mut command,
            &mut launch_env,
        ),
        ActorRuntime::Codex => {
            let setup = codex_mcp::configure(
                home,
                &group.group_id,
                &actor.id,
                &mut command,
                &mut launch_env,
            );
            codex_mcp::configure_mcp_only(
                home,
                &group.group_id,
                &actor.id,
                &mut command,
                &mut launch_env,
            );
            setup
        }
        ActorRuntime::Claude if hook_eligible => crate::ops::claude_hooks::configure(
            home,
            &group.group_id,
            &actor.id,
            cwd,
            &mut command,
            &mut launch_env,
        ),
        _ => {
            if let Err(error) =
                runtime_hook_session::prepare_identity(home, &group.group_id, &actor.id, None)
            {
                tracing::warn!(
                    %error,
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    "failed to clear stale runtime hook identity"
                );
            }
            return spawn(home, group, actor, cwd, command, launch_env);
        }
    };

    let mut setup = match setup {
        Ok(setup) => Some(setup),
        Err(error) => {
            tracing::warn!(
                %error,
                group_id = %group.group_id,
                actor_id = %actor.id,
                "runtime hook setup failed; launching without hook projection"
            );
            command = original_command.clone();
            launch_env = original_env.clone();
            None
        }
    };
    if let Err(error) =
        runtime_hook_session::prepare_identity(home, &group.group_id, &actor.id, setup.as_ref())
    {
        tracing::warn!(
            %error,
            group_id = %group.group_id,
            actor_id = %actor.id,
            "runtime hook identity preparation failed; launching without hook projection"
        );
        runtime_hook_session::revoke(&group.group_id, &actor.id);
        setup = None;
        command = original_command;
        launch_env = original_env;
    }

    match spawn(home, group, actor, cwd, command, launch_env) {
        Ok(status) => {
            if let Some(setup) = setup {
                if let Err(error) =
                    runtime_hook_session::bind(home, &group.group_id, &actor.id, &setup, &status)
                {
                    runtime_hook_session::revoke(&group.group_id, &actor.id);
                    tracing::warn!(
                        %error,
                        group_id = %group.group_id,
                        actor_id = %actor.id,
                        "runtime started but hook identity binding failed"
                    );
                }
            }
            Ok(status)
        }
        Err(error) => {
            if let Some(setup) = setup {
                let _ = codex_mcp::record_launch_issue(
                    home,
                    &setup.runtime,
                    &group.group_id,
                    &actor.id,
                    &setup.launch_token,
                    "HookUnavailableSpawn",
                );
            }
            Err(error)
        }
    }
}

fn spawn(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &Path,
    command: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<SessionStatus, OpError> {
    let history =
        super::terminal_history::config(home, &group.group_id, &actor.id).map_err(OpError::io)?;
    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: actor.runner,
            command,
            cwd: cwd.to_path_buf(),
            env,
            cols: 120,
            rows: 40,
        },
        history,
    )
    .map_err(super::runtime_error)
}

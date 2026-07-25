use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use cccc_runtime::{LaunchSpec, SessionStatus};
use std::path::PathBuf;

use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, actor_secrets, runtime_session};

mod persistence;
mod reconcile;
pub use persistence::persist_lifecycle;
pub use reconcile::{reap_exited, reconcile_exited};

pub fn apply(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    kind: &str,
) -> Result<Option<SessionStatus>, OpError> {
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("not_found", format!("actor not found: {actor_id}")))?;
    if is_structured(actor) {
        if super::local_headless::supports(actor) {
            match kind {
                "actor.stop" => super::local_headless::stop(&group.group_id, actor_id),
                "actor.restart" | "actor.new_session" => {
                    super::local_headless::stop(&group.group_id, actor_id);
                    start_local_headless(home, group, actor)?;
                }
                _ if !super::local_headless::running(&group.group_id, actor_id) => {
                    start_local_headless(home, group, actor)?;
                }
                _ => {}
            }
        } else {
            let _ = stop(group, actor_id)?;
        }
        return Ok(None);
    }
    match kind {
        "actor.stop" => stop(group, actor_id),
        "actor.restart" | "actor.new_session" => {
            let _ = stop(group, actor_id);
            start(home, group, actor).map(Some)
        }
        _ => match cccc_runtime::status(&group.group_id, actor_id) {
            Ok(status) if status.running => Ok(Some(status)),
            _ => start(home, group, actor).map(Some),
        },
    }
}

fn start_local_headless(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<(), OpError> {
    let mut actor = actor_profile_runtime::resolve(home, actor)?;
    let profile_secrets = actor_profile_runtime::profile_secrets(home, &actor)?;
    let actor_secret_values = actor_secrets::values(home, &group.group_id, &actor.id)?;
    actor.env.extend(profile_secrets);
    actor.env.extend(actor_secret_values);
    super::local_headless::start(home, group, &actor).map_err(OpError::io)
}

fn start(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<SessionStatus, OpError> {
    let actor = actor_profile_runtime::resolve(home, actor)?;
    let base_command = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let cwd = working_directory(group, &actor);
    let mut env = actor.env.clone();
    env.extend(actor_profile_runtime::profile_secrets(home, &actor)?);
    env.extend(actor_secrets::values(home, &group.group_id, &actor.id)?);
    let prepared = match (actor.runtime, actor.runner) {
        (ActorRuntime::Codex, cccc_contracts::RunnerKind::Pty) => {
            runtime_session::prepare_codex_command(
                home,
                &group.group_id,
                &actor.id,
                &cwd,
                &base_command,
                actor.runtime_state_source == RuntimeStateSource::AppServer,
            )
        }
        (ActorRuntime::Grok, cccc_contracts::RunnerKind::Pty) => {
            runtime_session::prepare_grok_command(
                home,
                &group.group_id,
                &actor.id,
                &cwd,
                &base_command,
            )
        }
        _ => runtime_session::PreparedCommand {
            command: base_command.clone(),
            resumed_session_id: None,
        },
    };
    let status = launch(home, group, &actor, &cwd, &env, prepared.command)?;

    if prepared.resumed_session_id.is_some() {
        schedule_resume_verification(
            home.clone(),
            group.clone(),
            actor.clone(),
            cwd,
            env,
            base_command,
            status.clone(),
        );
    } else {
        schedule_capture(home, group, &actor, cwd, base_command, &status);
    }
    Ok(status)
}

fn launch(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: &std::path::Path,
    env: &std::collections::BTreeMap<String, String>,
    mut command: Vec<String>,
) -> Result<SessionStatus, OpError> {
    let _start_permit = crate::runtime_start_gate::permit(home)
        .map_err(|message| OpError::new("runtime_shutting_down", message))?;
    let mut launch_env = env.clone();
    crate::ops::codex_mcp::configure_actor_cli(&mut launch_env);
    if actor.runtime == ActorRuntime::Codex {
        crate::ops::codex_mcp::configure(
            home,
            &group.group_id,
            &actor.id,
            &mut command,
            &mut launch_env,
        );
    }
    cccc_runtime::start(LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runner: actor.runner,
        command,
        cwd: cwd.to_path_buf(),
        env: launch_env,
        cols: 120,
        rows: 40,
    })
    .map_err(runtime_error)
}

fn schedule_capture(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    cwd: PathBuf,
    base_command: Vec<String>,
    status: &SessionStatus,
) {
    if actor.runtime == ActorRuntime::Codex
        && actor.runner == cccc_contracts::RunnerKind::Pty
        && status.running
    {
        runtime_session::schedule_codex_session_capture(
            home.clone(),
            group.group_id.clone(),
            actor.id.clone(),
            cwd,
            base_command,
            status.started_at.clone(),
        );
    }
}

fn schedule_resume_verification(
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    cwd: PathBuf,
    env: std::collections::BTreeMap<String, String>,
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

            if let Some(error) = error {
                let stopped = cccc_runtime::stop_if_started_at(
                    &group.group_id,
                    &actor.id,
                    &resumed_status.started_at,
                );
                if !matches!(stopped, Ok(Some(_))) {
                    return;
                }
                runtime_session::mark_resume_failed(&home, &group.group_id, &actor.id, &error);
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
                match launch(&home, &group, &actor, &cwd, &env, fresh_command) {
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
                return;
            }

            schedule_capture(&home, &group, &actor, cwd, base_command, &resumed_status);
        });
}

fn stop(group: &GroupDoc, actor_id: &str) -> Result<Option<SessionStatus>, OpError> {
    match cccc_runtime::stop(&group.group_id, actor_id) {
        Ok(status) => Ok(Some(status)),
        Err(cccc_runtime::RuntimeError::NotFound(_, _)) => Ok(None),
        Err(error) => Err(runtime_error(error)),
    }
}

pub fn status(group_id: &str, actor_id: &str) -> Option<SessionStatus> {
    cccc_runtime::status(group_id, actor_id).ok()
}

#[must_use]
pub fn is_structured(actor: &Actor) -> bool {
    actor.runner == RunnerKind::Headless || actor.runtime == ActorRuntime::WebModel
}

pub fn start_group(home: &HomeLayout, group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    let mut started = Vec::new();
    for actor in group.actors.iter().filter(|actor| actor.enabled) {
        match apply(home, group, &actor.id, "actor.start") {
            Ok(Some(status)) => started.push(status),
            Ok(None) => {}
            Err(error) => {
                for status in &started {
                    let _ = cccc_runtime::stop(&status.group_id, &status.actor_id);
                }
                return Err(error);
            }
        }
    }
    Ok(started)
}

pub fn stop_group(group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    super::local_headless::stop_group(&group.group_id);
    let mut stopped = Vec::new();
    for actor in &group.actors {
        if let Some(status) = stop(group, &actor.id)? {
            stopped.push(status);
        }
    }
    Ok(stopped)
}

fn working_directory(group: &GroupDoc, actor: &Actor) -> PathBuf {
    let wanted = if actor.default_scope_key.is_empty() {
        &group.active_scope_key
    } else {
        &actor.default_scope_key
    };
    group
        .scopes
        .iter()
        .find(|scope| &scope.scope_key == wanted)
        .or_else(|| group.scopes.first())
        .map(|scope| PathBuf::from(&scope.url))
        .filter(|path| path.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> OpError {
    OpError::new("runtime_error", error.to_string())
}

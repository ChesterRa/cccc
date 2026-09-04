use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use cccc_runtime::{LaunchSpec, SessionStatus};
use std::path::PathBuf;

use crate::dispatch::OpError;
use crate::ops::actor_profile_runtime;

mod environment;
mod persistence;
mod reconcile;
pub(crate) mod terminal_history;
pub use persistence::persist_lifecycle;
pub(crate) use reconcile::record_process_exit;
pub use reconcile::{reap_exited, reconcile_exited};

pub fn apply(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    kind: &str,
) -> Result<Option<SessionStatus>, OpError> {
    let stored_actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("not_found", format!("actor not found: {actor_id}")))?;
    let resolved_actor = if kind == "actor.stop" {
        None
    } else {
        Some(actor_profile_runtime::resolve(home, stored_actor)?)
    };
    let actor = resolved_actor.as_ref().unwrap_or(stored_actor);
    if kind != "actor.stop" {
        super::capabilities::apply_actor_startup_baseline(home, group, actor);
    }
    if actor.runtime == ActorRuntime::Deepseek {
        super::deepseek_runtime::apply(home, group, actor, kind)?;
        return Ok(None);
    }
    if super::local_headless::supports(actor) {
        match kind {
            "actor.stop" => {
                super::local_headless::stop(&group.group_id, actor_id).map_err(OpError::io)?
            }
            "actor.restart" | "actor.new_session" => {
                super::local_headless::stop(&group.group_id, actor_id).map_err(OpError::io)?;
                start_local_headless(home, group, actor)?;
            }
            _ if !super::local_headless::running(&group.group_id, actor_id) => {
                start_local_headless(home, group, actor)?;
            }
            _ => {}
        }
        return Ok(None);
    }
    if is_structured(actor) {
        let _ = stop(group, actor_id)?;
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
    let mut actor = environment::resolve_launch_actor(home, group, actor)?;
    let cwd = working_directory(group, &actor)?;
    let mut env = environment::launch_env(home, group, &actor);
    if super::local_headless::uses_managed_provider_cli(&actor) {
        super::runtime_mcp::prepare(home, actor.runtime, &cwd, &mut env)?;
    }
    actor.env = env;
    let _start_permit = crate::runtime_start_gate::permit(home)
        .map_err(|message| OpError::new("runtime_shutting_down", message))?;
    super::local_headless::start(home, group, &actor).map_err(OpError::io)
}

fn start(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<SessionStatus, OpError> {
    let actor = environment::resolve_launch_actor(home, group, actor)?;
    let command = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let cwd = working_directory(group, &actor)?;
    let mut env = environment::launch_env(home, group, &actor);
    super::runtime_mcp::prepare(home, actor.runtime, &cwd, &mut env)?;
    let _start_permit = crate::runtime_start_gate::permit(home)
        .map_err(|message| OpError::new("runtime_shutting_down", message))?;
    let command = cccc_runtime::resolve_command_executable(&command, &env);
    let history =
        terminal_history::config(home, &group.group_id, &actor.id).map_err(OpError::io)?;
    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command,
            cwd,
            env,
            cols: 120,
            rows: 40,
        },
        history,
    )
    .map_err(runtime_error)
}

pub(super) fn stop(group: &GroupDoc, actor_id: &str) -> Result<Option<SessionStatus>, OpError> {
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
    !super::local_headless::uses_managed_session(actor)
        && (actor.runner == RunnerKind::Headless || actor.runtime == ActorRuntime::WebModel)
}

pub fn start_group(home: &HomeLayout, group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    let mut statuses = Vec::new();
    let mut started_actor_ids = Vec::new();
    for actor in group.actors.iter().filter(|actor| actor.enabled) {
        let was_running = actor_is_running(group, actor);
        match apply(home, group, &actor.id, "actor.start") {
            Ok(status) => {
                if !was_running && actor_is_running(group, actor) {
                    started_actor_ids.push(actor.id.clone());
                }
                if let Some(status) = status {
                    statuses.push(status);
                }
            }
            Err(error) => {
                let mut rollback_failures = Vec::new();
                for actor_id in started_actor_ids.iter().rev() {
                    if let Err(rollback) = apply(home, group, actor_id, "actor.stop") {
                        rollback_failures.push(format!("{actor_id}: {}", rollback.message));
                    }
                }
                return if rollback_failures.is_empty() {
                    Err(error)
                } else {
                    Err(OpError::new(
                        "rollback_failed",
                        format!(
                            "{}; failed to stop newly started Actors: {}",
                            error.message,
                            rollback_failures.join("; ")
                        ),
                    ))
                };
            }
        }
    }
    Ok(statuses)
}

fn actor_is_running(group: &GroupDoc, actor: &Actor) -> bool {
    if super::local_headless::supports(actor) {
        super::local_headless::running(&group.group_id, &actor.id)
    } else {
        status(&group.group_id, &actor.id).is_some_and(|status| status.running)
    }
}

pub(crate) fn stop_all() -> Result<Vec<SessionStatus>, cccc_runtime::RuntimeError> {
    super::deepseek_runtime::stop_all();
    cccc_runtime::stop_all()
}

pub fn stop_group(group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    super::local_headless::stop_group(&group.group_id).map_err(OpError::io)?;
    super::deepseek_runtime::stop_group(&group.group_id);
    let mut stopped = Vec::new();
    for actor in &group.actors {
        if let Some(status) = stop(group, &actor.id)? {
            stopped.push(status);
        }
    }
    Ok(stopped)
}

pub(super) fn working_directory(group: &GroupDoc, actor: &Actor) -> Result<PathBuf, OpError> {
    let wanted = if actor.default_scope_key.is_empty() {
        &group.active_scope_key
    } else {
        &actor.default_scope_key
    };
    if wanted.is_empty() {
        return Err(OpError::new(
            "missing_project_root",
            "missing project root for group (no active scope)",
        ));
    }
    let scope = cccc_core::group_scope::resolve_attached_scope(group, wanted).ok_or_else(|| {
        OpError::new(
            "scope_not_attached",
            format!("scope not attached: {wanted}"),
        )
    })?;
    let path = PathBuf::from(&scope.url);
    if !path.is_dir() {
        return Err(OpError::new(
            "invalid_project_root",
            format!("project root path does not exist: {}", path.display()),
        ));
    }
    Ok(path)
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> OpError {
    OpError::new("runtime_error", error.to_string())
}

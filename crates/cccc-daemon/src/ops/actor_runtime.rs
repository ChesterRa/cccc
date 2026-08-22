use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use cccc_runtime::SessionStatus;
use std::path::PathBuf;

use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, runtime_session};

mod environment;
mod hook_launch;
#[cfg(test)]
mod hook_launch_tests;
mod persistence;
mod reconcile;
mod resume_verification;
pub(crate) mod terminal_history;
pub use persistence::persist_lifecycle;
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
    let base_command = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let cwd = working_directory(group, &actor)?;
    let mut env = environment::launch_env(home, group, &actor);
    super::runtime_mcp::prepare(home, actor.runtime, &cwd, &mut env)?;
    let prepared = match (actor.runtime, actor.runner) {
        (ActorRuntime::Codex, cccc_contracts::RunnerKind::Pty) => {
            runtime_session::prepare_codex_command(
                home,
                &group.group_id,
                &actor.id,
                &cwd,
                &base_command,
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
    super::runtime_hook_session::with_launch_lock(&group.group_id, &actor.id, || {
        let status =
            hook_launch::launch_serialized(home, group, &actor, &cwd, &env, prepared.command)?;
        if prepared.resumed_session_id.is_some() {
            resume_verification::schedule(
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
    })
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

pub(super) fn stop(group: &GroupDoc, actor_id: &str) -> Result<Option<SessionStatus>, OpError> {
    super::runtime_hook_session::with_launch_lock(&group.group_id, actor_id, || {
        resume_verification::cancel(&group.group_id, actor_id);
        match cccc_runtime::stop(&group.group_id, actor_id) {
            Ok(status) => {
                super::runtime_hook_session::revoke(&group.group_id, actor_id);
                super::runtime_hook_input::reset(&group.group_id, actor_id);
                Ok(Some(status))
            }
            Err(cccc_runtime::RuntimeError::NotFound(_, _)) => Ok(None),
            Err(error) => Err(runtime_error(error)),
        }
    })
}

#[cfg(test)]
pub(super) fn stop_if_started_at(
    group: &GroupDoc,
    status: &SessionStatus,
) -> Result<Option<SessionStatus>, OpError> {
    super::runtime_hook_session::with_launch_lock(&group.group_id, &status.actor_id, || {
        resume_verification::cancel_if_current(
            &group.group_id,
            &status.actor_id,
            &status.started_at,
        );
        match cccc_runtime::stop_if_started_at(
            &group.group_id,
            &status.actor_id,
            &status.started_at,
        ) {
            Ok(Some(stopped)) => {
                super::runtime_hook_session::revoke(&group.group_id, &status.actor_id);
                super::runtime_hook_input::reset(&group.group_id, &status.actor_id);
                Ok(Some(stopped))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(runtime_error(error)),
        }
    })
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
                    let _ = stop(group, &status.actor_id);
                }
                return Err(error);
            }
        }
    }
    Ok(started)
}

pub(crate) fn cancel_resume_verifications() {
    resume_verification::cancel_all();
}

pub(crate) fn stop_all() -> Result<Vec<SessionStatus>, cccc_runtime::RuntimeError> {
    cancel_resume_verifications();
    super::deepseek_runtime::stop_all();
    cccc_runtime::stop_all()
}

pub fn stop_group(group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    super::local_headless::stop_group(&group.group_id);
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

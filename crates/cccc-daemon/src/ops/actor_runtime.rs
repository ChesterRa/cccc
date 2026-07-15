use cccc_contracts::{Actor, ActorRuntime, Event, RuntimeStateSource};
use cccc_core::ledger;
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use cccc_runtime::{LaunchSpec, SessionStatus};
use std::path::PathBuf;

use crate::dispatch::OpError;
use crate::ops::{actor_delivery, actor_secrets, runtime_session};

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
    if actor.runtime == ActorRuntime::WebModel {
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

fn start(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<SessionStatus, OpError> {
    let base_command = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let cwd = working_directory(group, actor);
    let mut env = actor.env.clone();
    env.extend(actor_secrets::values(home, &group.group_id, &actor.id)?);
    let prepared = if actor.runtime == ActorRuntime::Codex
        && actor.runner == cccc_contracts::RunnerKind::Pty
    {
        runtime_session::prepare_codex_command(
            home,
            &group.group_id,
            &actor.id,
            &cwd,
            &base_command,
            actor.runtime_state_source == RuntimeStateSource::AppServer,
        )
    } else {
        runtime_session::PreparedCommand {
            command: base_command.clone(),
            resumed_session_id: None,
        }
    };
    let status = launch(home, group, actor, &cwd, &env, prepared.command)?;

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
        schedule_capture(home, group, actor, cwd, base_command, &status);
    }
    if status.running {
        actor_delivery::replay_unread(home, group, &actor.id);
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
    let mut launch_env = env.clone();
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
                match launch(&home, &group, &actor, &cwd, &env, base_command.clone()) {
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

pub fn start_group(home: &HomeLayout, group: &GroupDoc) -> Result<Vec<SessionStatus>, OpError> {
    let mut started = Vec::new();
    for actor in group
        .actors
        .iter()
        .filter(|actor| actor.enabled && actor.runtime != ActorRuntime::WebModel)
    {
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
    let mut stopped = Vec::new();
    for actor in &group.actors {
        if let Some(status) = stop(group, &actor.id)? {
            stopped.push(status);
        }
    }
    Ok(stopped)
}

pub fn reconcile(home: &HomeLayout) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for status in cccc_runtime::reap().map_err(runtime_error)? {
        let Ok(group) = store.load(&status.group_id) else {
            continue;
        };
        let Some(actor) = group
            .actors
            .iter()
            .find(|actor| actor.id == status.actor_id)
        else {
            continue;
        };
        if actor.runtime_state_source != RuntimeStateSource::Terminal {
            continue;
        }
        store
            .mutate(&status.group_id, |doc| {
                if let Some(actor) = doc
                    .actors
                    .iter_mut()
                    .find(|actor| actor.id == status.actor_id)
                {
                    actor.enabled = false;
                }
                doc.running = doc.actors.iter().any(|actor| actor.enabled);
                Ok(())
            })
            .map_err(OpError::io)?;
        let mut event = Event::new("actor.stop", &status.group_id);
        event.by = "system".into();
        event.data = serde_json::json!({
            "actor_id": status.actor_id,
            "reason": "process_exit",
            "exit_code": status.exit_code,
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(
            &store.ledger_path(&status.group_id).map_err(OpError::io)?,
            &event,
        )
        .map_err(OpError::io)?;
    }
    Ok(())
}

pub fn persist_lifecycle(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    enabled: bool,
    target_status: Option<&SessionStatus>,
) -> Result<Actor, OpError> {
    let running = group.actors.iter().any(|actor| {
        if actor.id == actor_id {
            enabled
                && (actor.runtime == ActorRuntime::WebModel
                    || target_status.is_some_and(|status| status.running))
        } else {
            actor.enabled
                && (actor.runtime == ActorRuntime::WebModel
                    || status(&group.group_id, &actor.id).is_some_and(|status| status.running))
        }
    });
    GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .mutate(&group.group_id, |doc| {
            let mut patch = serde_json::Map::new();
            patch.insert("enabled".into(), serde_json::Value::Bool(enabled));
            let actor = cccc_core::actors::update(doc, actor_id, &patch)?;
            doc.running = running;
            if enabled && doc.state == cccc_contracts::GroupState::Stopped {
                doc.state = cccc_contracts::GroupState::Active;
            } else if !running {
                doc.state = cccc_contracts::GroupState::Stopped;
            }
            Ok(actor)
        })
        .map_err(OpError::invalid)
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

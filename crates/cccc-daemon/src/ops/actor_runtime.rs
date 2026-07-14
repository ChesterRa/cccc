use cccc_contracts::{Actor, ActorRuntime, Event, RuntimeStateSource};
use cccc_core::ledger;
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use cccc_runtime::{LaunchSpec, SessionStatus};
use std::path::PathBuf;

use crate::dispatch::OpError;
use crate::ops::actor_secrets;

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
        "actor.restart" => {
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
    let command = if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    };
    let mut env = actor.env.clone();
    env.extend(actor_secrets::values(home, &group.group_id, &actor.id)?);
    cccc_runtime::start(LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runner: actor.runner,
        command,
        cwd: working_directory(group, actor),
        env,
        cols: 120,
        rows: 40,
    })
    .map_err(runtime_error)
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

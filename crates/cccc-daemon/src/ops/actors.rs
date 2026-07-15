use cccc_contracts::{Actor, DaemonRequest, Event};
use cccc_core::actors;
use cccc_core::ledger;
use cccc_core::permissions::{self, ActorAction};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, actor_runtime, actor_secrets, runtime_session};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "actor_list" => list(home, request),
        "actor_prompt" => prompt(home, request),
        "actor_add" => add(home, request),
        "actor_update" => update(home, request),
        "actor_remove" => remove(home, request),
        "actor_start" => lifecycle(home, request, "actor.start"),
        "actor_stop" => lifecycle(home, request, "actor.stop"),
        "actor_restart" => lifecycle(home, request, "actor.restart"),
        "actor_new_session" => lifecycle(home, request, "actor.new_session"),
        "actor_env_private_keys" => actor_secrets::keys(home, request),
        "actor_env_private_update" => actor_secrets::update(home, request),
        _ => return None,
    })
}

fn prompt(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))?;
    object(json!({
        "group_id":group.group_id,
        "actor_id":actor_id,
        "prompt":cccc_core::system_prompt::render(&group, actor)
    }))
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request, ActorAction::List, "")?;
    object(json!({"actors": actors_with_roles(home, &group)}))
}

fn add(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize(&group, request, ActorAction::Add, "")?;
    let actor = actor_from_args(request)?;
    let added = store(home)?
        .mutate(&group_id, |doc| actors::add(doc, actor))
        .map_err(OpError::invalid)?;
    append_event(
        home,
        &group_id,
        "actor.add",
        request,
        json!({"actor": added}),
    )?;
    object(json!({"actor": added}))
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize(&group, request, ActorAction::Update, &actor_id)?;
    let patch = request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| {
            request
                .args
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "actor_id" | "by"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });
    let actor = store(home)?
        .mutate(&group_id, |doc| actors::update(doc, &actor_id, &patch))
        .map_err(OpError::invalid)?;
    append_event(
        home,
        &group_id,
        "actor.update",
        request,
        json!({"actor_id": actor_id, "patch": patch}),
    )?;
    object(json!({"actor": actor}))
}

fn remove(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize(&group, request, ActorAction::Remove, &actor_id)?;
    actor_delivery::shutdown_actor(&group_id, &actor_id);
    actor_runtime::apply(home, &group, &actor_id, "actor.stop")?;
    let actor = store(home)?
        .mutate(&group_id, |doc| actors::remove(doc, &actor_id))
        .map_err(OpError::invalid)?;
    runtime_session::remove(home, &group_id, &actor_id);
    append_event(
        home,
        &group_id,
        "actor.remove",
        request,
        json!({"actor_id": actor_id}),
    )?;
    object(json!({"removed": true, "actor": actor}))
}

fn lifecycle(home: &HomeLayout, request: &DaemonRequest, kind: &str) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    let action = match kind {
        "actor.start" => ActorAction::Start,
        "actor.stop" => ActorAction::Stop,
        _ => ActorAction::Restart,
    };
    authorize(&group, request, action, &actor_id)?;
    if kind == "actor.new_session" {
        runtime_session::remove(home, &group_id, &actor_id);
    }
    if kind != "actor.start" {
        actor_delivery::shutdown_actor(&group_id, &actor_id);
    }
    let enabled = kind != "actor.stop";
    let status = actor_runtime::apply(home, &group, &actor_id, kind)?;
    let actor =
        actor_runtime::persist_lifecycle(home, &group, &actor_id, enabled, status.as_ref())?;
    append_event(
        home,
        &group_id,
        kind,
        request,
        json!({"actor_id": actor_id, "runner": actor.runner}),
    )?;
    object(json!({"actor": actor, "runtime": status}))
}

fn actor_from_args(request: &DaemonRequest) -> Result<Actor, OpError> {
    if let Some(value) = request.args.get("actor") {
        return serde_json::from_value(value.clone()).map_err(OpError::invalid);
    }
    let id = required_arg(request, "actor_id")?;
    let mut value = serde_json::to_value(Actor::new(id)).map_err(OpError::invalid)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| OpError::new("internal_error", "invalid actor"))?;
    for (key, item) in &request.args {
        if !matches!(key.as_str(), "group_id" | "actor_id" | "by") {
            object.insert(key.clone(), item.clone());
        }
    }
    serde_json::from_value(value).map_err(OpError::invalid)
}

fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}

fn authorize(
    group: &GroupDoc,
    request: &DaemonRequest,
    action: ActorAction,
    target: &str,
) -> Result<(), OpError> {
    permissions::require_actor(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        action,
        target,
    )
    .map_err(OpError::invalid)
}

fn append_event(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<(), OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}

fn actors_with_roles(home: &HomeLayout, group: &GroupDoc) -> Vec<Value> {
    group
        .actors
        .iter()
        .cloned()
        .map(|mut actor| {
            actor.role = actors::effective_role(group, &actor.id);
            let status = actor_runtime::status(&group.group_id, &actor.id);
            let mut value = serde_json::to_value(&actor).unwrap_or_else(|_| json!({}));
            if let Some(object) = value.as_object_mut() {
                object.extend(runtime_session::actor_fields(
                    home,
                    &group.group_id,
                    &actor.id,
                ));
                object.insert(
                    "running".into(),
                    Value::Bool(status.as_ref().is_some_and(|item| item.running)),
                );
                object.insert(
                    "pid".into(),
                    status
                        .and_then(|item| item.pid)
                        .map_or(Value::Null, |pid| Value::from(u64::from(pid))),
                );
            }
            value
        })
        .collect()
}

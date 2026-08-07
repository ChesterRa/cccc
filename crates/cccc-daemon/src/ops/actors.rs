use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::actors;
use cccc_core::ledger;
use cccc_core::permissions::{self, ActorAction};
use cccc_core::{GroupDoc, HomeLayout, group_scope};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{
    actor_delivery, actor_profile_runtime, actor_runtime, actor_secrets, runtime_session,
};

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
        "prompt":cccc_core::system_prompt::render_session(home, &group, actor)
    }))
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request, ActorAction::List, "")?;
    object(json!({"actors": super::actor_listing::list(home, &group, request)?}))
}

fn add(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize(&group, request, ActorAction::Add, "")?;
    let mut actor = actor_from_args(request)?;
    let private_env = private_env_arg(request)?;
    if !actor.profile_id.is_empty() {
        if private_env.is_some() {
            return Err(OpError::new(
                "actor_profile_linked_readonly",
                "env_private is profile-controlled for linked actors",
            ));
        }
        actor = actor_profile_runtime::link(home, &actor, &actor.profile_id)?;
    }
    if actor.runtime == ActorRuntime::WebModel {
        require_single_web_model_actor(home, &group_id, &actor.id)?;
    }
    actor.default_scope_key = normalize_default_scope_key(&group, &actor.default_scope_key)?;
    let added = store(home)?
        .mutate(&group_id, |doc| actors::add(doc, actor))
        .map_err(OpError::invalid)?;
    if let Some(values) = private_env
        && let Err(error) = actor_secrets::replace(home, &group_id, &added.id, values)
    {
        return Err(super::actor_saga::rollback_added(
            home, &group_id, &added.id, error,
        ));
    }
    if let Err(error) = append_event(
        home,
        &group_id,
        "actor.add",
        request,
        json!({"actor": added}),
    ) {
        return Err(super::actor_saga::rollback_added(
            home, &group_id, &added.id, error,
        ));
    }
    object(json!({"actor": added}))
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize(&group, request, ActorAction::Update, &actor_id)?;
    let profile_id = string_arg(request, "profile_id").unwrap_or_default();
    let profile_action = string_arg(request, "profile_action").unwrap_or_default();
    if !profile_id.is_empty() && !profile_action.is_empty() {
        return Err(OpError::new(
            "invalid_args",
            "profile_id and profile_action are mutually exclusive",
        ));
    }
    if !profile_action.is_empty() && profile_action != "convert_to_custom" {
        return Err(OpError::new("invalid_args", "invalid profile_action"));
    }
    let mut patch = request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| {
            request
                .args
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "group_id" | "actor_id" | "by" | "profile_id" | "profile_action"
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });
    if let Some(value) = patch.get("default_scope_key") {
        let reference = value
            .as_str()
            .ok_or_else(|| OpError::new("invalid_args", "default_scope_key must be a string"))?;
        patch.insert(
            "default_scope_key".into(),
            Value::String(normalize_default_scope_key(&group, reference)?),
        );
    }
    let current = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    if actor_profile_runtime::rejects_linked_patch(current, &patch) {
        return Err(OpError::new(
            "actor_profile_linked_readonly",
            "linked actor runtime fields are read-only; convert to custom first",
        ));
    }
    let mut preview_group = group.clone();
    let patched_preview =
        actors::update(&mut preview_group, &actor_id, &patch).map_err(OpError::invalid)?;
    let mut final_preview = if !profile_id.is_empty() {
        actor_profile_runtime::link(home, &patched_preview, &profile_id)?
    } else if profile_action == "convert_to_custom" {
        let mut resolved = actor_profile_runtime::resolve(home, &patched_preview)?;
        resolved.profile_id.clear();
        resolved.profile_revision_applied = 0;
        resolved
    } else {
        patched_preview
    };
    final_preview.role = None;
    if final_preview.runtime == ActorRuntime::WebModel {
        require_single_web_model_actor(home, &group_id, &actor_id)?;
    }
    let converted_secrets = if profile_action == "convert_to_custom" {
        let mut secrets = actor_profile_runtime::profile_secrets(home, current)?;
        secrets.extend(actor_secrets::values(home, &group_id, &actor_id)?);
        Some(secrets)
    } else {
        None
    };
    let actor = store(home)?
        .mutate(&group_id, |doc| {
            let patched = actors::update(doc, &actor_id, &patch)?;
            let mut final_actor = if !profile_id.is_empty() {
                actor_profile_runtime::link(home, &patched, &profile_id)
                    .map_err(|error| std::io::Error::other(error.message))?
            } else if profile_action == "convert_to_custom" {
                let mut resolved = actor_profile_runtime::resolve(home, &patched)
                    .map_err(|error| std::io::Error::other(error.message))?;
                resolved.profile_id.clear();
                resolved.profile_revision_applied = 0;
                resolved
            } else {
                patched
            };
            final_actor.role = None;
            let index = doc
                .actors
                .iter()
                .position(|actor| actor.id == actor_id)
                .ok_or_else(|| std::io::Error::other("actor not found"))?;
            doc.actors[index] = final_actor.clone();
            Ok(final_actor)
        })
        .map_err(OpError::invalid)?;
    if let Some(secrets) = converted_secrets {
        actor_secrets::replace(home, &group_id, &actor_id, secrets)?;
    }
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
    let index = group
        .actors
        .iter()
        .position(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    let original_actor = group.actors[index].clone();
    let original_secrets = actor_secrets::values(home, &group_id, &actor_id)?;
    actor_delivery::shutdown_actor(&group_id, &actor_id);
    actor_runtime::apply(home, &group, &actor_id, "actor.stop")?;
    let actor = store(home)?
        .mutate(&group_id, |doc| actors::remove(doc, &actor_id))
        .map_err(OpError::invalid)?;
    if let Err(error) = runtime_session::remove(home, &group_id, &actor_id).map_err(OpError::io) {
        return Err(super::actor_saga::restore_removed(
            home,
            &group_id,
            original_actor,
            index,
            original_secrets,
            error,
        ));
    }
    if let Err(error) = actor_secrets::remove(home, &group_id, &actor_id) {
        return Err(super::actor_saga::restore_removed(
            home,
            &group_id,
            original_actor,
            index,
            original_secrets,
            error,
        ));
    }
    if let Err(error) = append_event(
        home,
        &group_id,
        "actor.remove",
        request,
        json!({"actor_id": actor_id}),
    ) {
        return Err(super::actor_saga::restore_removed(
            home,
            &group_id,
            original_actor,
            index,
            original_secrets,
            error,
        ));
    }
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
        runtime_session::remove(home, &group_id, &actor_id).map_err(OpError::io)?;
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
        if !matches!(key.as_str(), "group_id" | "actor_id" | "by" | "env_private") {
            object.insert(key.clone(), item.clone());
        }
    }
    serde_json::from_value(value).map_err(OpError::invalid)
}

fn normalize_default_scope_key(group: &GroupDoc, reference: &str) -> Result<String, OpError> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Ok(String::new());
    }
    group_scope::resolve_attached_scope(group, reference)
        .map(|scope| scope.scope_key.clone())
        .ok_or_else(|| {
            OpError::new(
                "scope_not_attached",
                format!("scope not attached: {reference}"),
            )
        })
}

fn require_single_web_model_actor(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<(), OpError> {
    let store = store(home)?;
    for meta in store.list().map_err(OpError::io)? {
        let group = store.load(&meta.group_id).map_err(OpError::io)?;
        for actor in &group.actors {
            if actor.runtime != ActorRuntime::WebModel
                || (group.group_id == group_id && actor.id == actor_id)
            {
                continue;
            }
            let label = if actor.title.trim().is_empty() {
                actor.id.as_str()
            } else {
                actor.title.as_str()
            };
            return Err(OpError::new(
                "chatgpt_web_model_singleton",
                format!(
                    "ChatGPT Web Model is limited to one actor per CCCC instance (existing actor: {label} in group {}). Remove the existing ChatGPT Web Model actor before creating another.",
                    group.group_id
                ),
            ));
        }
    }
    Ok(())
}

fn private_env_arg(request: &DaemonRequest) -> Result<Option<BTreeMap<String, String>>, OpError> {
    let Some(value) = request.args.get("env_private") else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or_else(|| OpError::new("invalid_args", "env_private must be an object"))?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_owned()))
                .ok_or_else(|| OpError::new("invalid_args", "env_private values must be strings"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map(Some)
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

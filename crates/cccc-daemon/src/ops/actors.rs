use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, Event};
use cccc_core::actors;
use cccc_core::ledger;
use cccc_core::permissions::{self, ActorAction};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, group_scope, web_model_connectors};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{
    actor_delivery, actor_profile_runtime, actor_runtime, actor_secrets, runtime_session,
};

const WEB_MODEL_TARGETS_KEY: &str = "web_model_browser_targets";
const WEB_MODEL_DELIVERY_PREFERENCES_KEY: &str = "web_model_delivery_preferences";
const RUNTIME_STATES_KEY: &str = "runtime_states";

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
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if private_env.is_some() && !by.trim().is_empty() && by.trim() != "user" {
        return Err(OpError::new(
            "permission_denied",
            "env_private is only allowed for by=user",
        ));
    }
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
    if let Err(error) =
        web_model_connectors::retire_actor(home, &group_id, &added.id).map_err(OpError::io)
    {
        return Err(super::actor_saga::rollback_added(
            home, &group_id, &added.id, error,
        ));
    }
    if let Err(error) = remove_persisted_headless_state(home, &group_id, &added.id) {
        return Err(super::actor_saga::rollback_added(
            home,
            &group_id,
            &added.id,
            OpError::io(error),
        ));
    }
    if let Err(error) = actor_secrets::remove(home, &group_id, &added.id) {
        return Err(super::actor_saga::rollback_added(
            home, &group_id, &added.id, error,
        ));
    }
    if let Some(values) = private_env
        && let Err(error) = actor_secrets::replace(home, &group_id, &added.id, values)
    {
        return Err(super::actor_saga::rollback_added(
            home, &group_id, &added.id, error,
        ));
    }
    let event = match append_event(
        home,
        &group_id,
        "actor.add",
        request,
        json!({"actor": added}),
    ) {
        Ok(event) => event,
        Err(error) => {
            return Err(super::actor_saga::rollback_added(
                home, &group_id, &added.id, error,
            ));
        }
    };
    object(json!({"actor": added, "event": event}))
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
    let original_actor = current.clone();
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
        resolved.profile_scope = "global".into();
        resolved.profile_owner.clear();
        resolved.profile_revision_applied = 0;
        resolved
    } else {
        patched_preview
    };
    final_preview.role = None;
    if final_preview.runtime == ActorRuntime::WebModel {
        require_single_web_model_actor(home, &group_id, &actor_id)?;
    }
    let original_secrets = if profile_action == "convert_to_custom" {
        Some(actor_secrets::values(home, &group_id, &actor_id)?)
    } else {
        None
    };
    let converted_secrets = if profile_action == "convert_to_custom" {
        let mut secrets = actor_profile_runtime::profile_secrets(home, current)?;
        secrets.extend(original_secrets.clone().unwrap_or_default());
        Some(secrets)
    } else {
        None
    };
    let enabled_patched = patch.contains_key("enabled");
    let was_running = actor_process_running(&group, &original_actor);
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
                resolved.profile_scope = "global".into();
                resolved.profile_owner.clear();
                resolved.profile_revision_applied = 0;
                resolved
            } else {
                patched
            };
            final_actor.role = None;
            final_actor.normalize_runtime_constraints();
            let index = doc
                .actors
                .iter()
                .position(|actor| actor.id == actor_id)
                .ok_or_else(|| std::io::Error::other("actor not found"))?;
            doc.actors[index] = final_actor.clone();
            if enabled_patched && !final_actor.enabled {
                doc.running = doc.running && doc.actors.iter().any(|actor| actor.enabled);
            }
            Ok(final_actor)
        })
        .map_err(OpError::invalid)?;
    if let Some(secrets) = converted_secrets {
        if let Err(error) = actor_secrets::replace(home, &group_id, &actor_id, secrets) {
            return Err(rollback_actor_update(
                home,
                &group,
                &original_actor,
                original_secrets.as_ref(),
                None,
                ActorUpdateEffect::None,
                error,
            ));
        }
    }
    let updated_group =
        match store(home).and_then(|store| store.load(&group_id).map_err(OpError::io)) {
            Ok(group) => group,
            Err(error) => {
                return Err(rollback_actor_update(
                    home,
                    &group,
                    &original_actor,
                    original_secrets.as_ref(),
                    None,
                    ActorUpdateEffect::None,
                    error,
                ));
            }
        };
    let mut effect = ActorUpdateEffect::None;
    if enabled_patched && !actor.enabled {
        actor_delivery::shutdown_actor(&group_id, &actor_id);
        if let Err(error) = actor_runtime::apply(home, &group, &actor_id, "actor.stop") {
            let effect = if was_running && !actor_process_running(&group, &original_actor) {
                ActorUpdateEffect::Stopped
            } else {
                ActorUpdateEffect::None
            };
            return Err(rollback_actor_update(
                home,
                &group,
                &original_actor,
                original_secrets.as_ref(),
                Some(&updated_group),
                effect,
                error,
            ));
        }
        if was_running {
            effect = ActorUpdateEffect::Stopped;
        }
    } else if enabled_patched
        && actor.enabled
        && group.running
        && matches!(
            group.state,
            cccc_contracts::GroupState::Active | cccc_contracts::GroupState::Idle
        )
    {
        if let Err(error) = actor_runtime::apply(home, &updated_group, &actor_id, "actor.start") {
            let effect = if !was_running && actor_process_running(&updated_group, &actor) {
                ActorUpdateEffect::Started
            } else {
                ActorUpdateEffect::None
            };
            return Err(rollback_actor_update(
                home,
                &group,
                &original_actor,
                original_secrets.as_ref(),
                Some(&updated_group),
                effect,
                error,
            ));
        }
        if !was_running {
            effect = ActorUpdateEffect::Started;
        }
    }
    let event = match append_event(
        home,
        &group_id,
        "actor.update",
        request,
        json!({"actor_id": actor_id, "patch": patch}),
    ) {
        Ok(event) => event,
        Err(error) => {
            return Err(rollback_actor_update(
                home,
                &group,
                &original_actor,
                original_secrets.as_ref(),
                Some(&updated_group),
                effect,
                error,
            ));
        }
    };
    object(json!({"actor": actor, "event": event}))
}

#[derive(Clone, Copy)]
enum ActorUpdateEffect {
    None,
    Started,
    Stopped,
}

fn actor_process_running(group: &GroupDoc, actor: &Actor) -> bool {
    if super::local_headless::supports(actor) {
        super::local_headless::running(&group.group_id, &actor.id)
    } else if actor_runtime::is_structured(actor) {
        false
    } else {
        actor_runtime::status(&group.group_id, &actor.id).is_some_and(|status| status.running)
    }
}

fn rollback_actor_update(
    home: &HomeLayout,
    original_group: &GroupDoc,
    original_actor: &Actor,
    original_secrets: Option<&BTreeMap<String, String>>,
    updated_group: Option<&GroupDoc>,
    effect: ActorUpdateEffect,
    original: OpError,
) -> OpError {
    let mut failures = Vec::new();
    if matches!(effect, ActorUpdateEffect::Started)
        && let Some(group) = updated_group
    {
        actor_delivery::shutdown_actor(&group.group_id, &original_actor.id);
        if let Err(error) = actor_runtime::apply(home, group, &original_actor.id, "actor.stop") {
            failures.push(format!("stop newly started runtime: {}", error.message));
        }
    }
    match store(home).and_then(|store| {
        store
            .mutate(&original_group.group_id, |doc| {
                let index = doc
                    .actors
                    .iter()
                    .position(|actor| actor.id == original_actor.id)
                    .ok_or_else(|| std::io::Error::other("actor not found during rollback"))?;
                doc.actors[index] = original_actor.clone();
                doc.running = original_group.running;
                doc.state = original_group.state;
                Ok(())
            })
            .map_err(OpError::io)
    }) {
        Ok(()) => {}
        Err(error) => failures.push(format!("restore actor state: {}", error.message)),
    }
    if let Some(secrets) = original_secrets
        && let Err(error) = actor_secrets::replace(
            home,
            &original_group.group_id,
            &original_actor.id,
            secrets.clone(),
        )
    {
        failures.push(format!("restore actor secrets: {}", error.message));
    }
    if matches!(effect, ActorUpdateEffect::Stopped)
        && let Err(error) =
            actor_runtime::apply(home, original_group, &original_actor.id, "actor.start")
    {
        failures.push(format!(
            "restart previously running actor: {}",
            error.message
        ));
    }
    if failures.is_empty() {
        original
    } else {
        OpError::new(
            "rollback_failed",
            format!(
                "{}; rollback failed: {}",
                original.message,
                failures.join("; ")
            ),
        )
    }
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
    let original_web_model_target = web_model_target(&group, &actor_id).cloned();
    let original_web_model_delivery_preference =
        web_model_delivery_preference(&group, &actor_id).cloned();
    let original_runtime_state = runtime_state(&group, &actor_id).cloned();
    let original_secrets = actor_secrets::values(home, &group_id, &actor_id)?;
    store(home)?
        .mutate(&group_id, |doc| {
            actors::remove(doc, &actor_id)?;
            Ok(())
        })
        .map_err(OpError::invalid)?;
    let retired_connectors = match web_model_connectors::retire_actor(home, &group_id, &actor_id) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(super::actor_saga::restore_removed(
                home,
                &group_id,
                super::actor_saga::RemovedActorSnapshot {
                    actor: original_actor,
                    index,
                    web_model_target: original_web_model_target,
                    web_model_delivery_preference: original_web_model_delivery_preference,
                    runtime_state: original_runtime_state,
                    connector_entries: Vec::new(),
                    secrets: original_secrets,
                },
                OpError::io(error),
            ));
        }
    };
    let removal_snapshot = super::actor_saga::RemovedActorSnapshot {
        actor: original_actor,
        index,
        web_model_target: original_web_model_target,
        web_model_delivery_preference: original_web_model_delivery_preference,
        runtime_state: original_runtime_state,
        connector_entries: retired_connectors,
        secrets: original_secrets,
    };
    if let Err(error) = actor_secrets::remove(home, &group_id, &actor_id) {
        return Err(super::actor_saga::restore_removed(
            home,
            &group_id,
            removal_snapshot,
            error,
        ));
    }
    let event = match append_event(
        home,
        &group_id,
        "actor.remove",
        request,
        json!({"actor_id": actor_id}),
    ) {
        Ok(event) => event,
        Err(error) => {
            return Err(super::actor_saga::restore_removed(
                home,
                &group_id,
                removal_snapshot,
                error,
            ));
        }
    };
    actor_delivery::shutdown_actor(&group_id, &actor_id);
    if let Err(error) = actor_runtime::apply(home, &group, &actor_id, "actor.stop") {
        tracing::warn!(
            message = %error.message,
            %group_id,
            %actor_id,
            "post-commit actor runtime stop failed"
        );
    }
    if let Err(error) = remove_persisted_headless_state(home, &group_id, &actor_id) {
        tracing::warn!(%error, %group_id, %actor_id, "post-commit headless state cleanup failed");
    }
    if let Err(error) = runtime_session::remove(home, &group_id, &actor_id) {
        tracing::warn!(%error, %group_id, %actor_id, "post-commit runtime session cleanup failed");
    }
    object(json!({"actor_id": actor_id, "event": event}))
}

fn web_model_target<'a>(group: &'a GroupDoc, actor_id: &str) -> Option<&'a Value> {
    group
        .extra
        .get(WEB_MODEL_TARGETS_KEY)
        .and_then(Value::as_object)
        .and_then(|targets| targets.get(actor_id))
}

fn web_model_delivery_preference<'a>(group: &'a GroupDoc, actor_id: &str) -> Option<&'a Value> {
    group
        .extra
        .get(WEB_MODEL_DELIVERY_PREFERENCES_KEY)
        .and_then(Value::as_object)
        .and_then(|preferences| preferences.get(actor_id))
}

fn runtime_state<'a>(group: &'a GroupDoc, actor_id: &str) -> Option<&'a Value> {
    group
        .extra
        .get(RUNTIME_STATES_KEY)
        .and_then(Value::as_object)
        .and_then(|states| states.get(actor_id))
}

fn remove_persisted_headless_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> std::io::Result<()> {
    let path = home
        .groups_dir()
        .join(group_id)
        .join("state/runners/headless")
        .join(format!("{actor_id}.json"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
    let original_actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .cloned()
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    let runtime_was_running = actor_process_running(&group, &original_actor);
    let runtime_session_snapshot = if kind == "actor.new_session" {
        Some(runtime_session::snapshot(home, &group_id, &actor_id).map_err(OpError::io)?)
    } else {
        None
    };
    if kind == "actor.new_session" {
        runtime_session::remove(home, &group_id, &actor_id).map_err(OpError::io)?;
    }
    if kind != "actor.start" || !runtime_was_running {
        actor_delivery::shutdown_actor(&group_id, &actor_id);
    }
    let enabled = kind != "actor.stop";
    let status = match actor_runtime::apply(home, &group, &actor_id, kind) {
        Ok(status) => status,
        Err(error) => {
            let effect = lifecycle_effect(
                kind,
                runtime_was_running,
                actor_process_running(&group, &original_actor),
            );
            return Err(rollback_actor_lifecycle(
                home,
                &group,
                &original_actor,
                runtime_session_snapshot.as_ref(),
                effect,
                error,
            ));
        }
    };
    let effect = lifecycle_effect(
        kind,
        runtime_was_running,
        actor_process_running(&group, &original_actor),
    );
    let actor =
        match actor_runtime::persist_lifecycle(home, &group, &actor_id, enabled, status.as_ref()) {
            Ok(actor) => actor,
            Err(error) => {
                return Err(rollback_actor_lifecycle(
                    home,
                    &group,
                    &original_actor,
                    runtime_session_snapshot.as_ref(),
                    effect,
                    error,
                ));
            }
        };
    let event = match append_event(
        home,
        &group_id,
        kind,
        request,
        json!({"actor_id": actor_id, "runner": actor.runner}),
    ) {
        Ok(event) => event,
        Err(error) => {
            return Err(rollback_actor_lifecycle(
                home,
                &group,
                &original_actor,
                runtime_session_snapshot.as_ref(),
                effect,
                error,
            ));
        }
    };
    if enabled {
        match GroupStore::new(home.clone()).and_then(|store| store.load(&group_id)) {
            Ok(current_group) => {
                actor_delivery::dispatch_unread(home, &current_group, &actor_id);
            }
            Err(error) => tracing::warn!(
                %error,
                %group_id,
                %actor_id,
                "failed to reload actor inbox after activation"
            ),
        }
    }
    object(json!({"actor": actor, "event": event, "runtime": status}))
}

#[derive(Clone, Copy)]
enum ActorLifecycleEffect {
    None,
    Started,
    Stopped,
    Replaced,
}

fn lifecycle_effect(kind: &str, was_running: bool, is_running: bool) -> ActorLifecycleEffect {
    match (kind, was_running, is_running) {
        ("actor.stop", true, false) => ActorLifecycleEffect::Stopped,
        ("actor.start", false, true) => ActorLifecycleEffect::Started,
        ("actor.restart" | "actor.new_session", true, true) => ActorLifecycleEffect::Replaced,
        ("actor.restart" | "actor.new_session", true, false) => ActorLifecycleEffect::Stopped,
        ("actor.restart" | "actor.new_session", false, true) => ActorLifecycleEffect::Started,
        _ => ActorLifecycleEffect::None,
    }
}

fn rollback_actor_lifecycle(
    home: &HomeLayout,
    original_group: &GroupDoc,
    original_actor: &Actor,
    runtime_session_snapshot: Option<&Option<serde_json::Map<String, Value>>>,
    effect: ActorLifecycleEffect,
    original: OpError,
) -> OpError {
    let mut failures = Vec::new();
    if matches!(
        effect,
        ActorLifecycleEffect::Started | ActorLifecycleEffect::Replaced
    ) {
        actor_delivery::shutdown_actor(&original_group.group_id, &original_actor.id);
        if let Err(error) =
            actor_runtime::apply(home, original_group, &original_actor.id, "actor.stop")
        {
            failures.push(format!("stop replacement runtime: {}", error.message));
        }
    }
    if let Some(snapshot) = runtime_session_snapshot
        && let Err(error) = runtime_session::restore_snapshot(
            home,
            &original_group.group_id,
            &original_actor.id,
            snapshot.as_ref(),
        )
    {
        failures.push(format!("restore runtime session: {error}"));
    }
    match store(home).and_then(|store| {
        store
            .mutate(&original_group.group_id, |doc| {
                let index = doc
                    .actors
                    .iter()
                    .position(|actor| actor.id == original_actor.id)
                    .ok_or_else(|| std::io::Error::other("actor not found during rollback"))?;
                doc.actors[index] = original_actor.clone();
                doc.running = original_group.running;
                doc.state = original_group.state;
                Ok(())
            })
            .map_err(OpError::io)
    }) {
        Ok(()) => {}
        Err(error) => failures.push(format!("restore actor state: {}", error.message)),
    }
    if matches!(
        effect,
        ActorLifecycleEffect::Stopped | ActorLifecycleEffect::Replaced
    ) && let Err(error) =
        actor_runtime::apply(home, original_group, &original_actor.id, "actor.start")
    {
        failures.push(format!("restore previous runtime: {}", error.message));
    }
    if original_actor.enabled && original_group.running {
        actor_delivery::dispatch_unread(home, original_group, &original_actor.id);
    }
    if failures.is_empty() {
        original
    } else {
        OpError::new(
            "rollback_failed",
            format!(
                "{}; rollback failed: {}",
                original.message,
                failures.join("; ")
            ),
        )
    }
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
    if object.len() > 256 {
        return Err(OpError::new("invalid_args", "too many env_private keys"));
    }
    let mut values = BTreeMap::new();
    for (key, value) in object {
        actor_secrets::validate_env_key(key)?;
        let value = actor_secrets::python_string(value)?;
        if value.chars().count() > 200_000 {
            return Err(OpError::new("invalid_args", "env value too large"));
        }
        values.insert(key.clone(), value);
    }
    Ok(Some(values))
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
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}

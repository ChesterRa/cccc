use cccc_contracts::{DaemonRequest, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Registry, active, permissions};
use serde_json::json;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub(super) fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    if required_arg(request, "confirm")? != group_id {
        return Err(OpError::new(
            "confirm_required",
            format!("confirm must equal group_id: {group_id}"),
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let old = store.load(&group_id).map_err(OpError::not_found)?;
    permissions::require_group(
        &old,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(OpError::invalid)?;
    let was_active = active::get(home).map_err(OpError::io)?.as_deref() == Some(&group_id);
    let created = store.create(&old.title, &old.topic).map_err(OpError::io)?;
    let replacement = prepare_replacement(&store, &old, &created)
        .and_then(|replacement| {
            super::actor_secrets::copy_group(home, &old.group_id, &replacement.group_id)?;
            super::groups::append_group_event(
                home,
                &replacement,
                "group.create",
                request,
                json!({
                    "title":replacement.title,
                    "topic":replacement.topic,
                    "reset_from":old.group_id
                }),
            )?;
            Ok(replacement)
        })
        .map_err(|error| rollback_new(&store, &old, &created.group_id, error))?;

    super::actor_delivery::shutdown_group(&old.group_id);
    if let Err(error) = super::actor_runtime::stop_group(&old) {
        return Err(rollback_new(&store, &old, &replacement.group_id, error));
    }
    if was_active && let Err(error) = active::set(home, &replacement.group_id).map_err(OpError::io)
    {
        return Err(rollback_new(&store, &old, &replacement.group_id, error));
    }
    let old_delete_error = store
        .delete(&old.group_id)
        .map_err(OpError::io)
        .err()
        .map(|error| error.message);
    if old_delete_error.is_none() {
        super::actor_secrets::remove_group(home, &old.group_id)?;
    }
    object(json!({
        "old_group_id":old.group_id,
        "new_group_id":replacement.group_id,
        "group_id":replacement.group_id,
        "deleted_old":old_delete_error.is_none(),
        "old_delete_error":old_delete_error,
        "active_group_id":was_active.then_some(&replacement.group_id),
        "group":super::group_runtime::group(replacement),
    }))
}

fn prepare_replacement(
    store: &GroupStore,
    old: &GroupDoc,
    created: &GroupDoc,
) -> Result<GroupDoc, OpError> {
    let new_group_id = created.group_id.clone();
    let created_at = created.created_at.clone();
    let replacement = store
        .mutate(&new_group_id, |document| {
            *document = old.clone();
            document.group_id.clone_from(&new_group_id);
            document.created_at.clone_from(&created_at);
            document.running = false;
            document.state = GroupState::Active;
            for actor in &mut document.actors {
                actor.avatar_asset_path.clear();
            }
            Ok(document.clone())
        })
        .map_err(OpError::io)?;
    Registry::mutate(store.home(), |registry| {
        if let Some(meta) = registry.groups.get_mut(&new_group_id) {
            meta.default_scope_key
                .clone_from(&replacement.active_scope_key);
        }
        for scope in &replacement.scopes {
            registry
                .defaults
                .insert(scope.scope_key.clone(), new_group_id.clone());
        }
        Ok(())
    })
    .map_err(OpError::io)?;
    Ok(replacement)
}

fn rollback_new(store: &GroupStore, old: &GroupDoc, group_id: &str, original: OpError) -> OpError {
    if let Err(rollback) = store.delete(group_id) {
        return OpError::new(
            "rollback_failed",
            format!(
                "{}; failed to remove replacement {group_id}: {rollback}",
                original.message
            ),
        );
    }
    match Registry::mutate(store.home(), |registry| {
        for scope in &old.scopes {
            registry
                .defaults
                .insert(scope.scope_key.clone(), old.group_id.clone());
        }
        Ok(())
    }) {
        Ok(()) => original,
        Err(rollback) => OpError::new(
            "rollback_failed",
            format!(
                "{}; failed to restore scope registry for {}: {rollback}",
                original.message, old.group_id
            ),
        ),
    }
}

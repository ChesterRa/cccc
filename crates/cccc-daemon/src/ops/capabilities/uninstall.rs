use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::capabilities::CapabilityStore;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use cccc_core::profiles::ProfileStore;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg};

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let capability_id = required_arg(request, "capability_id")?;
    let actor = super::actor_context(home, request)?;
    super::authorize_group_admin(&actor, "uninstall a capability")?;
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let store = CapabilityStore::new(home.clone());
    let removed_bindings = store
        .remove_bindings_for_group(&capability_id, &actor.group_id)
        .map_err(OpError::io)?;
    let removed_group_marker = store
        .set_removed_for_group(&capability_id, &actor.group_id, true)
        .map_err(OpError::io)?;
    let removed_actor_autoload = remove_actor_autoload(&groups, &actor.group_id, &capability_id)?;
    let retain_installation = store.has_bindings(&capability_id).map_err(OpError::io)?;
    let runtime = cleanup_runtime(home, &actor.group_id, &capability_id, retain_installation)?;
    let refresh_required = removed_bindings > 0
        || removed_group_marker
        || runtime.removed_installation
        || runtime.removed_bindings > 0
        || runtime.removed_recent_success
        || removed_actor_autoload > 0;

    object(json!({
        "action_id":format!("cun_{}", &Uuid::new_v4().simple().to_string()[..16]),
        "group_id":actor.group_id,"actor_id":actor.actor_id,"capability_id":capability_id,
        "state":"ready","removed_record":false,
        "removed_bindings":removed_bindings,"removed_blocked":0,
        "removed_group_marker":removed_group_marker,
        "removed_installation":runtime.removed_installation,
        "removed_runtime_bindings":runtime.removed_bindings,
        "removed_recent_success":runtime.removed_recent_success,
        "removed_actor_autoload":removed_actor_autoload,
        "removed_profile_autoload":0,
        "cleanup_skipped_reason":if retain_installation {"cleanup_skipped_capability_still_bound"} else {""},
        "refresh_required":refresh_required,
        "refresh_mode":if refresh_required {"relist_or_reconnect"} else {""},
        "wait":if refresh_required {"relist_or_reconnect"} else {""}
    }))
}

fn remove_actor_autoload(
    groups: &GroupStore,
    group_id: &str,
    capability_id: &str,
) -> Result<usize, OpError> {
    groups
        .mutate(group_id, |group| {
            let mut count = 0;
            for actor in &mut group.actors {
                let before = actor.capability_autoload.len();
                actor
                    .capability_autoload
                    .retain(|item| item != capability_id);
                count += before - actor.capability_autoload.len();
            }
            Ok(count)
        })
        .map_err(OpError::io)
}

#[derive(Default)]
pub(super) struct GlobalCleanup {
    pub removed_runtime_bindings: usize,
    pub removed_installations: usize,
    pub removed_recent_success: usize,
    pub removed_actor_autoload: usize,
    pub removed_profile_autoload: usize,
}

pub(super) fn cleanup_global_references(
    home: &HomeLayout,
    capability_ids: &[String],
) -> Result<GlobalCleanup, OpError> {
    if capability_ids.is_empty() {
        return Ok(GlobalCleanup::default());
    }
    let targets = capability_ids.iter().cloned().collect::<BTreeSet<_>>();
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let mut removed_actor_autoload = 0;
    for group in groups.list().map_err(OpError::io)? {
        removed_actor_autoload += groups
            .mutate(&group.group_id, |doc| {
                let mut count = 0;
                for actor in &mut doc.actors {
                    let before = actor.capability_autoload.len();
                    actor
                        .capability_autoload
                        .retain(|item| !targets.contains(item));
                    count += before - actor.capability_autoload.len();
                }
                Ok(count)
            })
            .map_err(OpError::io)?;
    }
    let profiles = ProfileStore::new(home.clone()).map_err(OpError::io)?;
    let mut removed_profile_autoload = 0;
    for capability_id in capability_ids {
        removed_profile_autoload += profiles
            .remove_capability_default(capability_id)
            .map_err(OpError::io)?;
    }
    let (removed_runtime_bindings, removed_installations, removed_recent_success) =
        cleanup_runtime_globally(home, capability_ids)?;
    Ok(GlobalCleanup {
        removed_runtime_bindings,
        removed_installations,
        removed_recent_success,
        removed_actor_autoload,
        removed_profile_autoload,
    })
}

#[derive(Default)]
struct RuntimeCleanup {
    removed_bindings: usize,
    removed_installation: bool,
    removed_recent_success: bool,
}

fn cleanup_runtime(
    home: &HomeLayout,
    group_id: &str,
    capability_id: &str,
    retain_installation: bool,
) -> Result<RuntimeCleanup, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok(RuntimeCleanup::default());
    }
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut runtime: Value = read_json(&path)?;
        let removed_bindings =
            remove_runtime_bindings(runtime.get_mut("actor_instances"), group_id, capability_id);
        let mut removed_installation = false;
        let mut removed_recent_success = false;
        let mut changed = removed_bindings > 0;
        if !retain_installation {
            let (removed_artifact_binding, installation_removed) =
                remove_runtime_artifact(&mut runtime, capability_id);
            removed_installation = installation_removed;
            removed_recent_success = remove_runtime_recent_success(&mut runtime, capability_id);
            changed |= removed_artifact_binding || removed_recent_success;
        }
        if changed || removed_installation {
            runtime["updated_at"] = json!(utc_now());
            write_json(&path, &runtime)?;
        }
        Ok(RuntimeCleanup {
            removed_bindings,
            removed_installation,
            removed_recent_success,
        })
    })
    .map_err(OpError::io)
}

fn cleanup_runtime_globally(
    home: &HomeLayout,
    capability_ids: &[String],
) -> Result<(usize, usize, usize), OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok((0, 0, 0));
    }
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut runtime: Value = read_json(&path)?;
        let mut removed_bindings = 0;
        let mut removed_installations = 0;
        let mut removed_recent_success = 0;
        let mut changed = false;
        for capability_id in capability_ids {
            removed_bindings += remove_runtime_bindings_all_groups(
                runtime.get_mut("actor_instances"),
                capability_id,
            );
            let (removed_artifact_binding, removed_installation) =
                remove_runtime_artifact(&mut runtime, capability_id);
            changed |= removed_artifact_binding;
            removed_installations += usize::from(removed_installation);
            removed_recent_success +=
                usize::from(remove_runtime_recent_success(&mut runtime, capability_id));
        }
        changed |= removed_bindings > 0 || removed_recent_success > 0;
        if changed {
            runtime["updated_at"] = json!(utc_now());
            write_json(&path, &runtime)?;
        }
        Ok((
            removed_bindings,
            removed_installations,
            removed_recent_success,
        ))
    })
    .map_err(OpError::io)
}

fn remove_runtime_artifact(runtime: &mut Value, capability_id: &str) -> (bool, bool) {
    let artifact_id = runtime
        .get_mut("capability_artifacts")
        .and_then(Value::as_object_mut)
        .and_then(|items| items.remove(capability_id))
        .and_then(|value| value.as_str().map(str::to_owned));
    let Some(artifact_id) = artifact_id else {
        return (false, false);
    };
    if let Some(capability_ids) = runtime
        .get_mut("artifacts")
        .and_then(Value::as_object_mut)
        .and_then(|items| items.get_mut(&artifact_id))
        .and_then(|artifact| artifact.get_mut("capability_ids"))
        .and_then(Value::as_array_mut)
    {
        capability_ids.retain(|value| value.as_str() != Some(capability_id));
    }
    let referenced = runtime["capability_artifacts"]
        .as_object()
        .is_some_and(|items| items.values().any(|value| value == &artifact_id));
    let removed_installation = !referenced
        && runtime
            .get_mut("artifacts")
            .and_then(Value::as_object_mut)
            .is_some_and(|items| items.remove(&artifact_id).is_some());
    (true, removed_installation)
}

fn remove_runtime_recent_success(runtime: &mut Value, capability_id: &str) -> bool {
    runtime
        .get_mut("recent_success")
        .and_then(Value::as_object_mut)
        .is_some_and(|items| items.remove(capability_id).is_some())
}

fn remove_runtime_bindings(
    actor_instances: Option<&mut Value>,
    group_id: &str,
    capability_id: &str,
) -> usize {
    let Some(groups) = actor_instances.and_then(Value::as_object_mut) else {
        return 0;
    };
    let mut removed = 0;
    if let Some(actors) = groups.get_mut(group_id).and_then(Value::as_object_mut) {
        for capabilities in actors.values_mut().filter_map(Value::as_object_mut) {
            removed += usize::from(capabilities.remove(capability_id).is_some());
        }
    }
    removed
}

fn remove_runtime_bindings_all_groups(
    actor_instances: Option<&mut Value>,
    capability_id: &str,
) -> usize {
    let Some(groups) = actor_instances.and_then(Value::as_object_mut) else {
        return 0;
    };
    groups
        .values_mut()
        .filter_map(Value::as_object_mut)
        .flat_map(|actors| actors.values_mut().filter_map(Value::as_object_mut))
        .map(|capabilities| usize::from(capabilities.remove(capability_id).is_some()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_cleanup_only_removes_selected_capability() {
        let mut value = json!({"g":{"a":{"skill:one":{},"skill:two":{}}}});
        assert_eq!(
            remove_runtime_bindings(Some(&mut value), "g", "skill:one"),
            1
        );
        assert!(value["g"]["a"].get("skill:one").is_none());
        assert!(value["g"]["a"].get("skill:two").is_some());
    }
}

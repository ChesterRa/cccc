use cccc_contracts::{ActorRole, DaemonRequest, utc_now};
use cccc_core::actors::effective_role;
use cccc_core::capabilities::CapabilityStore;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use cccc_core::profiles::ProfileStore;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let capability_id = required_arg(request, "capability_id")?;
    let by = string_arg(request, "by")
        .or_else(|| string_arg(request, "actor_id"))
        .unwrap_or_else(|| "user".into());
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = groups.load(&group_id).map_err(OpError::not_found)?;
    if by != "user" && effective_role(&group, &by) != Some(ActorRole::Foreman) {
        return Err(OpError::new(
            "permission_denied",
            "only user or foreman can uninstall a capability",
        ));
    }

    let store = CapabilityStore::new(home.clone());
    let record = store.catalog_record(&capability_id).map_err(OpError::io)?;
    let generated = record
        .as_ref()
        .and_then(|value| value["source_id"].as_str())
        .is_some_and(is_generated_source);
    let removed_record = if generated {
        store.remove_record(&capability_id).map_err(OpError::io)?
    } else {
        false
    };
    let removed_bindings = if generated {
        store
            .remove_all_bindings(&capability_id)
            .map_err(OpError::io)?
    } else {
        store
            .remove_bindings_for_group(&capability_id, &group_id)
            .map_err(OpError::io)?
    };
    let removed_actor_autoload =
        remove_actor_autoload(&groups, &group_id, &capability_id, generated)?;
    let removed_profile_autoload = if generated {
        ProfileStore::new(home.clone())
            .map_err(OpError::io)?
            .remove_capability_default(&capability_id)
            .map_err(OpError::io)?
    } else {
        0
    };
    let retain_installation = store.has_bindings(&capability_id).map_err(OpError::io)?;
    let runtime = cleanup_runtime(
        home,
        &group_id,
        &capability_id,
        generated,
        retain_installation,
    )?;

    object(json!({
        "action_id":format!("cun_{}", &Uuid::new_v4().simple().to_string()[..16]),
        "group_id":group_id,"actor_id":by,"capability_id":capability_id,
        "state":"ready","removed_record":removed_record,
        "removed_bindings":removed_bindings,"removed_blocked":0,
        "removed_group_marker":false,
        "removed_installation":runtime.removed_installation,
        "removed_runtime_bindings":runtime.removed_bindings,
        "removed_recent_success":runtime.removed_recent_success,
        "removed_actor_autoload":removed_actor_autoload,
        "removed_profile_autoload":removed_profile_autoload,
        "cleanup_skipped_reason":if retain_installation {"cleanup_skipped_capability_still_bound"} else {""},
        "refresh_required":true,"refresh_mode":"relist_or_reconnect","wait":"relist_or_reconnect"
    }))
}

fn is_generated_source(source: &str) -> bool {
    matches!(
        source,
        "manual_import" | "agent_self_proposed" | "github_import" | "url_import" | "local_import"
    )
}

fn remove_actor_autoload(
    groups: &GroupStore,
    group_id: &str,
    capability_id: &str,
    all_groups: bool,
) -> Result<usize, OpError> {
    let ids = if all_groups {
        groups
            .list()
            .map_err(OpError::io)?
            .into_iter()
            .map(|meta| meta.group_id)
            .collect()
    } else {
        vec![group_id.to_owned()]
    };
    let mut removed = 0;
    for id in ids {
        removed += groups
            .mutate(&id, |group| {
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
            .map_err(OpError::io)?;
    }
    Ok(removed)
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
    all_groups: bool,
    retain_installation: bool,
) -> Result<RuntimeCleanup, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok(RuntimeCleanup::default());
    }
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut runtime: Value = read_json(&path)?;
        let removed_bindings = remove_runtime_bindings(
            runtime.get_mut("actor_instances"),
            group_id,
            capability_id,
            all_groups,
        );
        let mut removed_installation = false;
        let mut removed_recent_success = false;
        let mut changed = removed_bindings > 0;
        if !retain_installation {
            let artifact_id = runtime
                .get_mut("capability_artifacts")
                .and_then(Value::as_object_mut)
                .and_then(|items| items.remove(capability_id))
                .and_then(|value| value.as_str().map(str::to_owned));
            if let Some(artifact_id) = artifact_id {
                changed = true;
                if let Some(capability_ids) = runtime
                    .pointer_mut(&format!("/artifacts/{artifact_id}/capability_ids"))
                    .and_then(Value::as_array_mut)
                {
                    capability_ids.retain(|value| value.as_str() != Some(capability_id));
                }
                let referenced = runtime["capability_artifacts"]
                    .as_object()
                    .is_some_and(|items| items.values().any(|value| value == &artifact_id));
                if !referenced {
                    removed_installation = runtime
                        .get_mut("artifacts")
                        .and_then(Value::as_object_mut)
                        .is_some_and(|items| items.remove(&artifact_id).is_some());
                }
            }
            removed_recent_success = runtime
                .get_mut("recent_success")
                .and_then(Value::as_object_mut)
                .is_some_and(|items| items.remove(capability_id).is_some());
            changed |= removed_recent_success;
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

fn remove_runtime_bindings(
    actor_instances: Option<&mut Value>,
    group_id: &str,
    capability_id: &str,
    all_groups: bool,
) -> usize {
    let Some(groups) = actor_instances.and_then(Value::as_object_mut) else {
        return 0;
    };
    let target_groups = if all_groups {
        groups.keys().cloned().collect::<Vec<_>>()
    } else {
        vec![group_id.to_owned()]
    };
    let mut removed = 0;
    for id in target_groups {
        if let Some(actors) = groups.get_mut(&id).and_then(Value::as_object_mut) {
            for capabilities in actors.values_mut().filter_map(Value::as_object_mut) {
                removed += usize::from(capabilities.remove(capability_id).is_some());
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_cleanup_only_removes_selected_capability() {
        let mut value = json!({"g":{"a":{"skill:one":{},"skill:two":{}}}});
        assert_eq!(
            remove_runtime_bindings(Some(&mut value), "g", "skill:one", false),
            1
        );
        assert!(value["g"]["a"].get("skill:one").is_none());
        assert!(value["g"]["a"].get("skill:two").is_some());
    }
}

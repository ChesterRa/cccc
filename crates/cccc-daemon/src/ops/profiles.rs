use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::profiles::ProfileStore;
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::actor_secrets;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "actor_profile_list" => list(home),
        "actor_profile_get" => get(home, request),
        "actor_profile_upsert" => upsert(home, request),
        "actor_profile_delete" => delete(home, request),
        "actor_profile_env_private_keys" => secret_keys(home, request),
        "actor_profile_env_private_update" => secret_update(home, request),
        "actor_profile_copy_actor_secrets" => copy_actor(home, request),
        "actor_profile_copy_profile_secrets" => copy_profile(home, request),
        _ => return None,
    })
}

fn store(home: &HomeLayout) -> Result<ProfileStore, OpError> {
    ProfileStore::new(home.clone()).map_err(OpError::io)
}
fn list(home: &HomeLayout) -> OpResult {
    object(json!({"profiles":store(home)?.list().map_err(OpError::io)?}))
}
fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get(&profile_id)
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    object(json!({"profile":profile,"usage":profiles.usage(&profile_id).map_err(OpError::io)?}))
}
fn upsert(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let mut profile = request
        .args
        .get("profile")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| {
            request
                .args
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "by" | "expected_revision" | "scope" | "owner_id"
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>()
        });
    for key in ["scope", "owner_id"] {
        if let Some(value) = request.args.get(key) {
            profile.insert(key.into(), value.clone());
        }
    }
    if !profile.contains_key("id")
        && let Some(profile_id) = profile.remove("profile_id")
    {
        profile.insert("id".into(), profile_id);
    }
    let expected = request
        .args
        .get("expected_revision")
        .and_then(Value::as_u64);
    let profile = store(home)?
        .upsert(profile, expected)
        .map_err(OpError::invalid)?;
    object(json!({"profile":profile}))
}
fn delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let (deleted, detached) = store(home)?
        .delete(&profile_id, bool_arg(request, "force_detach", false))
        .map_err(OpError::invalid)?;
    object(
        json!({"deleted":deleted,"profile_id":profile_id,"detached_count":detached.len(),"detached":detached}),
    )
}
fn secret_keys(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let keys = store(home)?.secret_keys(&profile_id).map_err(OpError::io)?;
    let masked = keys
        .iter()
        .map(|key| (key.clone(), json!("********")))
        .collect::<Map<_, _>>();
    object(json!({"profile_id":profile_id,"keys":keys,"masked_values":masked}))
}
fn secret_update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let empty = Map::new();
    let set = request
        .args
        .get("set")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let empty_unset = Vec::new();
    let unset = request
        .args
        .get("unset")
        .and_then(Value::as_array)
        .unwrap_or(&empty_unset);
    let keys = store(home)?
        .update_secrets(&profile_id, set, unset, bool_arg(request, "clear", false))
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"keys":keys}))
}
fn copy_actor(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let values = actor_secrets::values(home, &group_id, &actor_id)?;
    let keys = store(home)?
        .replace_secrets(&profile_id, values)
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"group_id":group_id,"actor_id":actor_id,"keys":keys}))
}
fn copy_profile(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let source = string_arg(request, "source_profile_id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "source_profile_id is required"))?;
    let values = store(home)?.secret_values(&source).map_err(OpError::io)?;
    let keys = store(home)?
        .replace_secrets(&profile_id, values)
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"source_profile_id":source,"keys":keys}))
}

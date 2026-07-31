use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::profiles::ProfileStore;
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::actor_secrets;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "actor_profile_list" => list(home, request),
        "actor_profile_get" => get(home, request),
        "actor_profile_upsert" => upsert(home, request),
        "actor_profile_delete" => delete(home, request),
        "actor_profile_env_private_keys" | "actor_profile_secret_keys" => {
            secret_keys(home, request)
        }
        "actor_profile_env_private_update" | "actor_profile_secret_update" => {
            secret_update(home, request)
        }
        "actor_profile_copy_actor_secrets" | "actor_profile_secret_copy_from_actor" => {
            copy_actor(home, request)
        }
        "actor_profile_copy_profile_secrets" | "actor_profile_secret_copy_from_profile" => {
            copy_profile(home, request)
        }
        _ => return None,
    })
}

fn store(home: &HomeLayout) -> Result<ProfileStore, OpError> {
    ProfileStore::new(home.clone()).map_err(OpError::io)
}
fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profiles = store(home)?.list().map_err(OpError::io)?;
    object(json!({"profiles":super::profile_access::list(request, profiles)?}))
}
fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_read(request, &profile)?;
    object(json!({
        "profile":profile,
        "usage":profiles
            .usage_ref(&profile_id,&profile_scope(request),&profile_owner(request))
            .map_err(OpError::io)?
    }))
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
    let profiles = store(home)?;
    let existing = profile
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| {
            profiles.get_ref(
                id,
                profile
                    .get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or("global"),
                profile
                    .get("owner_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        })
        .transpose()
        .map_err(OpError::io)?
        .flatten();
    if let Some(existing) = existing {
        super::profile_access::require_write(request, &existing)?;
        for field in ["scope", "owner_id"] {
            if profile
                .get(field)
                .is_some_and(|value| value != &existing[field])
            {
                return Err(OpError::new(
                    "permission_denied",
                    "profile scope and owner cannot be changed",
                ));
            }
            profile.insert(field.into(), existing[field].clone());
        }
    } else {
        super::profile_access::normalize_new(request, &mut profile)?;
    }
    let expected = request
        .args
        .get("expected_revision")
        .and_then(Value::as_u64);
    let profile = profiles
        .upsert(profile, expected)
        .map_err(OpError::invalid)?;
    object(json!({"profile":profile}))
}
fn delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_write(request, &profile)?;
    let (deleted, detached) = profiles
        .delete_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            bool_arg(request, "force_detach", false),
        )
        .map_err(OpError::invalid)?;
    object(
        json!({"deleted":deleted,"profile_id":profile_id,"detached_count":detached.len(),"detached":detached}),
    )
}
fn secret_keys(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_read(request, &profile)?;
    let keys = profiles
        .secret_keys_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?;
    let masked = keys
        .iter()
        .map(|key| (key.clone(), json!("********")))
        .collect::<Map<_, _>>();
    object(json!({"profile_id":profile_id,"keys":keys,"masked_values":masked}))
}
fn secret_update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_write(request, &profile)?;
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
    let keys = profiles
        .update_secrets_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            set,
            unset,
            bool_arg(request, "clear", false),
        )
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"keys":keys}))
}
fn copy_actor(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let profiles = store(home)?;
    let profile = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_write(request, &profile)?;
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    super::profile_access::require_group(request, &group_id)?;
    let group = crate::dispatch::store(home)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    if !group.actors.iter().any(|actor| actor.id == actor_id) {
        return Err(OpError::new("actor_not_found", "actor not found"));
    }
    let values = actor_secrets::values(home, &group_id, &actor_id)?;
    let keys = profiles
        .replace_secrets_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            values,
        )
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"group_id":group_id,"actor_id":actor_id,"keys":keys}))
}
fn copy_profile(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let profile_id = required_arg(request, "profile_id")?;
    let source = string_arg(request, "source_profile_id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "source_profile_id is required"))?;
    let profiles = store(home)?;
    let target = profiles
        .get_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "profile not found"))?;
    super::profile_access::require_write(request, &target)?;
    let source_scope =
        string_arg(request, "source_profile_scope").unwrap_or_else(|| "global".into());
    let source_owner = string_arg(request, "source_profile_owner").unwrap_or_default();
    let source_profile = profiles
        .get_ref(&source, &source_scope, &source_owner)
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("not_found", "source profile not found"))?;
    super::profile_access::require_read(request, &source_profile)?;
    let values = profiles
        .secret_values_ref(&source, &source_scope, &source_owner)
        .map_err(OpError::io)?;
    let keys = profiles
        .replace_secrets_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            values,
        )
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"source_profile_id":source,"keys":keys}))
}

fn profile_scope(request: &DaemonRequest) -> String {
    string_arg(request, "profile_scope")
        .or_else(|| string_arg(request, "scope"))
        .unwrap_or_else(|| "global".into())
}

fn profile_owner(request: &DaemonRequest) -> String {
    string_arg(request, "profile_owner")
        .or_else(|| string_arg(request, "owner_id"))
        .unwrap_or_default()
}

use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use cccc_contracts::{ActorRuntime, DaemonRequest};
use cccc_core::profiles::ProfileStore;
use cccc_core::{GroupStore, HomeLayout, actors};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::actor_secrets;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "actor_profile_list" => Operation::new(Read, list),
        "actor_profile_get" => Operation::new(Read, get),
        "actor_profile_upsert" => Operation::new(Write, upsert),
        "actor_profile_delete" => Operation::new(Write, delete),
        "actor_profile_env_private_keys" | "actor_profile_secret_keys" => {
            Operation::new(Read, secret_keys)
        }
        "actor_profile_env_private_update" | "actor_profile_secret_update" => {
            Operation::new(Write, secret_update)
        }
        "actor_profile_copy_actor_secrets" | "actor_profile_secret_copy_from_actor" => {
            Operation::new(Write, copy_actor)
        }
        "actor_profile_copy_profile_secrets" | "actor_profile_secret_copy_from_profile" => {
            Operation::new(Write, copy_profile)
        }
        "actor_profile_copy_voice_analyst_secrets" => Operation::new(Write, copy_voice_analyst),
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
    require_web_model_singleton(home, &profiles, &profile)?;
    let profile = profiles
        .upsert(profile, expected)
        .map_err(OpError::invalid)?;
    object(json!({"profile":profile}))
}

/// Linked actors run with the profile runtime, so a profile that switches to
/// ChatGPT Web Model is bound by the same instance-wide limit as actor_add.
fn require_web_model_singleton(
    home: &HomeLayout,
    profiles: &ProfileStore,
    profile: &Map<String, Value>,
) -> Result<(), OpError> {
    let runtime = profile
        .get("runtime")
        .cloned()
        .map(serde_json::from_value::<ActorRuntime>);
    let profile_id = profile
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(runtime, Some(Ok(ActorRuntime::WebModel))) || profile_id.is_empty() {
        return Ok(());
    }
    let usage = profiles
        .usage_ref(
            profile_id,
            profile
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("global"),
            profile
                .get("owner_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .map_err(OpError::io)?;
    let linked = usage
        .iter()
        .filter_map(|entry| Some((entry["group_id"].as_str()?, entry["actor_id"].as_str()?)))
        .collect::<Vec<_>>();
    if linked.len() > 1 {
        return Err(OpError::new(
            "chatgpt_web_model_singleton",
            format!(
                "ChatGPT Web Model is limited to one actor per CCCC instance; profile {profile_id} is linked to {} actors",
                linked.len()
            ),
        ));
    }
    if linked.is_empty() {
        return Ok(());
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    match actors::web_model_singleton_conflict(&store, linked.first().copied())
        .map_err(OpError::io)?
    {
        Some(message) => Err(OpError::new("chatgpt_web_model_singleton", message)),
        _ => Ok(()),
    }
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
    let usage = profiles
        .usage_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
        )
        .map_err(OpError::io)?;
    let force_detach = bool_arg(request, "force_detach", false);
    if !usage.is_empty() && !force_detach {
        return Err(OpError::new(
            "profile_in_use",
            "profile is in use; force_detach is required",
        ));
    }
    if force_detach {
        for linked in &usage {
            let group_id = linked["group_id"]
                .as_str()
                .ok_or_else(|| OpError::new("invalid_state", "profile usage has no group_id"))?;
            let actor_id = linked["actor_id"]
                .as_str()
                .ok_or_else(|| OpError::new("invalid_state", "profile usage has no actor_id"))?;
            let mut args = Map::new();
            for key in ["by", "caller_id", "is_admin", "allowed_groups"] {
                if let Some(value) = request.args.get(key) {
                    args.insert(key.into(), value.clone());
                }
            }
            args.insert("group_id".into(), json!(group_id));
            args.insert("actor_id".into(), json!(actor_id));
            args.insert("profile_action".into(), json!("convert_to_custom"));
            args.insert("patch".into(), json!({}));
            let conversion = DaemonRequest {
                v: request.v,
                op: "actor_update".into(),
                args,
            };
            super::actors::resolve_operation(&conversion)
                .ok_or_else(|| OpError::new("unknown_op", "actor_update is unavailable"))?
                .execute(home, &conversion)?;
        }
    }
    let (deleted, _) = profiles
        .delete_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            false,
        )
        .map_err(OpError::invalid)?;
    object(
        json!({"deleted":deleted,"profile_id":profile_id,"detached_count":usage.len(),"detached":usage}),
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
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "actor not found"))?;
    if !actor.profile_id.is_empty() {
        let source = profiles
            .get_ref(
                &actor.profile_id,
                &actor.profile_scope,
                &actor.profile_owner,
            )
            .map_err(OpError::io)?
            .ok_or_else(|| OpError::new("profile_not_found", "profile not found"))?;
        let source_request = {
            let mut source_request = request.clone();
            source_request
                .args
                .insert("profile_scope".into(), json!(actor.profile_scope));
            source_request
                .args
                .insert("profile_owner".into(), json!(actor.profile_owner));
            source_request
        };
        super::profile_access::require_read(&source_request, &source)?;
    }
    let values = actor_secrets::effective_values(home, &group_id, actor)?;
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

fn copy_voice_analyst(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if !bool_arg(request, "is_admin", false) {
        return Err(OpError::new(
            "permission_denied",
            "administrator access is required to copy Voice Analyst secrets",
        ));
    }
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
    let values = cccc_core::codex_voice_settings::private_environment(home).map_err(OpError::io)?;
    let keys = profiles
        .replace_secrets_ref(
            &profile_id,
            &profile_scope(request),
            &profile_owner(request),
            values,
        )
        .map_err(OpError::io)?;
    object(json!({"profile_id":profile_id,"keys":keys}))
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

use cccc_contracts::Actor;
use cccc_core::HomeLayout;
use cccc_core::profiles::ProfileStore;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::dispatch::OpError;

const CONTROLLED_FIELDS: &[&str] = &[
    "runtime",
    "runner",
    "command",
    "submit",
    "env",
    "capability_autoload",
];

pub fn rejects_linked_patch(actor: &Actor, patch: &serde_json::Map<String, Value>) -> bool {
    !actor.profile_id.is_empty()
        && patch
            .keys()
            .any(|key| CONTROLLED_FIELDS.contains(&key.as_str()))
}

pub fn link(home: &HomeLayout, actor: &Actor, profile_id: &str) -> Result<Actor, OpError> {
    let mut linked = apply(home, actor, profile_id)?;
    linked.profile_id = profile_id.to_owned();
    Ok(linked)
}

pub fn resolve(home: &HomeLayout, actor: &Actor) -> Result<Actor, OpError> {
    if actor.profile_id.is_empty() {
        return Ok(actor.clone());
    }
    apply(home, actor, &actor.profile_id)
}

pub fn profile_secrets(
    home: &HomeLayout,
    actor: &Actor,
) -> Result<BTreeMap<String, String>, OpError> {
    if actor.profile_id.is_empty() {
        return Ok(BTreeMap::new());
    }
    ProfileStore::new(home.clone())
        .map_err(OpError::io)?
        .secret_values_ref(
            &actor.profile_id,
            &actor.profile_scope,
            &actor.profile_owner,
        )
        .map_err(OpError::io)
}

fn apply(home: &HomeLayout, actor: &Actor, profile_id: &str) -> Result<Actor, OpError> {
    let profile = ProfileStore::new(home.clone())
        .map_err(OpError::io)?
        .get_ref(profile_id, &actor.profile_scope, &actor.profile_owner)
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "profile_not_found",
                format!("profile not found: {profile_id}"),
            )
        })?;
    let mut resolved = actor.clone();
    if let Some(value) = profile.get("runtime") {
        resolved.runtime = parse(value, "profile runtime")?;
    }
    if let Some(value) = profile.get("runner") {
        resolved.runner = parse(value, "profile runner")?;
    }
    if let Some(value) = profile.get("submit") {
        resolved.submit = parse(value, "profile submit")?;
    }
    if let Some(command) = profile.get("command") {
        resolved.command = command
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    }
    if let Some(items) = profile
        .pointer("/capability_defaults/autoload_capabilities")
        .and_then(Value::as_array)
    {
        resolved.capability_autoload = items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    resolved.profile_revision_applied = profile["revision"].as_u64().unwrap_or(0);
    Ok(resolved)
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value, field: &str) -> Result<T, OpError> {
    serde_json::from_value(value.clone())
        .map_err(|error| OpError::new("invalid_profile", format!("{field}: {error}")))
}

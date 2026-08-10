use cccc_contracts::Actor;
use cccc_core::HomeLayout;
use cccc_core::profiles::ProfileStore;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::dispatch::OpError;

const CONTROLLED_FIELDS: &[&str] = &[
    "runtime",
    "runner",
    "command",
    "submit",
    "env",
    "capability_autoload",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDefaults {
    pub autoload_capabilities: Vec<String>,
    pub default_scope: String,
    pub session_ttl_seconds: i64,
}

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

pub fn capability_defaults(
    home: &HomeLayout,
    actor: &Actor,
) -> Result<Option<CapabilityDefaults>, OpError> {
    if actor.profile_id.is_empty() {
        return Ok(None);
    }
    let profile = ProfileStore::new(home.clone())
        .map_err(OpError::io)?
        .get_ref(
            &actor.profile_id,
            &actor.profile_scope,
            &actor.profile_owner,
        )
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "profile_not_found",
                format!("profile not found: {}", actor.profile_id),
            )
        })?;
    let defaults = profile.get("capability_defaults");
    let mut seen = BTreeSet::new();
    let autoload_capabilities = defaults
        .and_then(|value| value.get("autoload_capabilities"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_owned()))
        .map(str::to_owned)
        .collect();
    let default_scope = defaults
        .and_then(|value| value.get("default_scope"))
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "actor" | "session"))
        .unwrap_or("actor")
        .to_owned();
    let session_ttl_seconds = defaults
        .and_then(|value| value.get("session_ttl_seconds"))
        .and_then(Value::as_i64)
        .unwrap_or(3600)
        .clamp(60, 86_400);
    Ok(Some(CapabilityDefaults {
        autoload_capabilities,
        default_scope,
        session_ttl_seconds,
    }))
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
    resolved.profile_revision_applied = profile["revision"].as_u64().unwrap_or(0);
    Ok(resolved)
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value, field: &str) -> Result<T, OpError> {
    serde_json::from_value(value.clone())
        .map_err(|error| OpError::new("invalid_profile", format!("{field}: {error}")))
}

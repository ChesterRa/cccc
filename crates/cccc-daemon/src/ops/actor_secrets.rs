use cccc_contracts::DaemonRequest;
use cccc_core::fs::{read_json, write_secret_json};
use cccc_core::{GroupStore, HomeLayout};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::dispatch::{OpError, OpResult, object, required_arg};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SecretStore {
    #[serde(default)]
    actors: BTreeMap<String, BTreeMap<String, String>>,
}

pub fn keys(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let state = load(home, &group_id)?;
    let keys: Vec<_> = state
        .actors
        .get(&actor_id)
        .into_iter()
        .flat_map(|values| values.keys())
        .cloned()
        .collect();
    object(json!({"actor_id": actor_id, "keys": keys}))
}

pub fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let mut state = load(home, &group_id)?;
    let values = state.actors.entry(actor_id.clone()).or_default();
    if request
        .args
        .get("clear")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        values.clear();
    }
    if let Some(set) = request.args.get("set").and_then(Value::as_object) {
        for (key, value) in set {
            let secret = value
                .as_str()
                .ok_or_else(|| OpError::new("invalid_args", "secret values must be strings"))?;
            values.insert(key.clone(), secret.into());
        }
    }
    if let Some(unset) = request.args.get("unset").and_then(Value::as_array) {
        for key in unset.iter().filter_map(Value::as_str) {
            values.remove(key);
        }
    }
    let keys: Vec<_> = values.keys().cloned().collect();
    write_secret_json(&path(home, &group_id)?, &state).map_err(OpError::io)?;
    object(json!({"actor_id": actor_id, "keys": keys, "updated": true}))
}

fn load(home: &HomeLayout, group_id: &str) -> Result<SecretStore, OpError> {
    let path = path(home, group_id)?;
    if path.exists() {
        read_json(&path).map_err(OpError::io)
    } else {
        Ok(SecretStore::default())
    }
}

pub fn values(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<BTreeMap<String, String>, OpError> {
    Ok(load(home, group_id)?
        .actors
        .remove(actor_id)
        .unwrap_or_default())
}

pub fn replace(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    values: BTreeMap<String, String>,
) -> Result<(), OpError> {
    let mut state = load(home, group_id)?;
    if values.is_empty() {
        state.actors.remove(actor_id);
    } else {
        state.actors.insert(actor_id.to_owned(), values);
    }
    write_secret_json(&path(home, group_id)?, &state).map_err(OpError::io)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<(), OpError> {
    replace(home, group_id, actor_id, BTreeMap::new())
}

fn path(home: &HomeLayout, group_id: &str) -> Result<PathBuf, OpError> {
    Ok(GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .state_dir(group_id)
        .map_err(OpError::io)?
        .join("actor-secrets.json"))
}

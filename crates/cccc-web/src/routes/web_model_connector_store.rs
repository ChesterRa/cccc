use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, fs, settings};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

#[cfg(test)]
use cccc_core::integration_state;

use crate::AppState;
use crate::api::ApiError;

const LEGACY_SETTINGS_KEY: &str = "web_model_connectors";

fn store_path(home: &HomeLayout) -> PathBuf {
    home.root().join("web_model_connectors.yaml")
}

fn lock_path(home: &HomeLayout) -> PathBuf {
    store_path(home).with_extension("yaml.lock")
}

fn hash_secret(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

fn secret_preview(secret: &str) -> String {
    if secret.chars().count() <= 10 {
        return "****".into();
    }
    let prefix = secret.chars().take(6).collect::<String>();
    let suffix = secret
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn normalized_entry(connector_id: &str, raw: &Value) -> Option<Value> {
    let connector_id = connector_id.trim();
    let mut item = raw.as_object()?.clone();
    let group_id = item.get("group_id")?.as_str()?.trim();
    let actor_id = item.get("actor_id")?.as_str()?.trim();
    if connector_id.is_empty() || group_id.is_empty() || actor_id.is_empty() {
        return None;
    }
    let secret = item
        .get("secret")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let secret_hash = item
        .get("secret_hash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if secret.is_empty() && secret_hash.is_empty() {
        return None;
    }
    item.insert("connector_id".into(), json!(connector_id));
    item.entry("kind")
        .or_insert_with(|| json!("web_model_connector"));
    if secret_hash.is_empty() {
        item.insert("secret_hash".into(), json!(hash_secret(&secret)));
    }
    if item
        .get("secret_preview")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
        && !secret.is_empty()
    {
        item.insert("secret_preview".into(), json!(secret_preview(&secret)));
    }
    item.entry("revoked").or_insert_with(|| Value::Bool(false));
    Some(Value::Object(item))
}

fn connector_map(raw: &Value) -> Map<String, Value> {
    let mut result = Map::new();
    if let Some(items) = raw.as_array() {
        for item in items {
            let id = item["connector_id"].as_str().unwrap_or("");
            if let Some(item) = normalized_entry(id, item) {
                result.insert(id.to_owned(), item);
            }
        }
        return collapse_active_duplicates(result);
    }
    let Some(root) = raw.as_object() else {
        return result;
    };
    let items = root
        .get("connectors")
        .and_then(Value::as_object)
        .unwrap_or(root);
    for (id, item) in items {
        if let Some(item) = normalized_entry(id, item) {
            result.insert(id.clone(), item);
        }
    }
    collapse_active_duplicates(result)
}

fn entry_rank(item: &Value, connector_id: &str) -> (String, String, String, String) {
    (
        item["created_at"].as_str().unwrap_or("").to_owned(),
        item["updated_at"].as_str().unwrap_or("").to_owned(),
        item["last_activity_at"].as_str().unwrap_or("").to_owned(),
        connector_id.to_owned(),
    )
}

fn collapse_active_duplicates(mut connectors: Map<String, Value>) -> Map<String, Value> {
    let mut current_by_actor = BTreeMap::<(String, String), String>::new();
    for (connector_id, item) in &connectors {
        if item["revoked"].as_bool().unwrap_or(false) {
            continue;
        }
        let group_id = item["group_id"].as_str().unwrap_or("").to_owned();
        let actor_id = item["actor_id"].as_str().unwrap_or("").to_owned();
        if group_id.is_empty() || actor_id.is_empty() {
            continue;
        }
        let key = (group_id, actor_id);
        let replace = current_by_actor
            .get(&key)
            .and_then(|current_id| {
                connectors
                    .get(current_id)
                    .map(|current| entry_rank(item, connector_id) > entry_rank(current, current_id))
            })
            .unwrap_or(true);
        if replace {
            current_by_actor.insert(key, connector_id.clone());
        }
    }
    let current_ids = current_by_actor
        .into_values()
        .collect::<std::collections::BTreeSet<_>>();
    for (connector_id, item) in &mut connectors {
        if item["revoked"].as_bool().unwrap_or(false) || current_ids.contains(connector_id) {
            continue;
        }
        item["revoked"] = Value::Bool(true);
        if item["updated_at"].as_str().unwrap_or("").is_empty() {
            item["updated_at"] = item["created_at"].clone();
        }
    }
    connectors
}

fn merge_maps(
    mut canonical: Map<String, Value>,
    imported: Map<String, Value>,
) -> Map<String, Value> {
    for (connector_id, incoming) in imported {
        let Some(existing) = canonical.get(&connector_id) else {
            canonical.insert(connector_id, incoming);
            continue;
        };
        let mut merged =
            if entry_rank(&incoming, &connector_id) > entry_rank(existing, &connector_id) {
                existing.as_object().cloned().unwrap_or_default()
            } else {
                incoming.as_object().cloned().unwrap_or_default()
            };
        let preferred =
            if entry_rank(&incoming, &connector_id) > entry_rank(existing, &connector_id) {
                incoming.as_object()
            } else {
                existing.as_object()
            };
        if let Some(preferred) = preferred {
            merged.extend(preferred.clone());
        }
        canonical.insert(connector_id, Value::Object(merged));
    }
    collapse_active_duplicates(canonical)
}

fn read_unlocked(path: &Path) -> io::Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    Ok(connector_map(&fs::read_yaml::<Value>(path)?))
}

fn write_unlocked(path: &Path, connectors: &Map<String, Value>) -> io::Result<()> {
    fs::write_secret_yaml(path, &json!({"connectors":connectors}))
}

fn migrate_settings_store(home: &HomeLayout) -> io::Result<()> {
    // `settings::load` performs its own legacy JSON migration under the same
    // settings lock. Complete that migration before entering our read/modify/
    // write section so a legacy home cannot self-deadlock here.
    settings::load(home)?;
    fs::with_exclusive_lock(&home.root().join("settings.yaml.lock"), || {
        let mut global = settings::load(home)?;
        let Some(legacy) = global.extra.get(LEGACY_SETTINGS_KEY).cloned() else {
            return Ok(());
        };
        let imported = connector_map(&legacy);
        fs::with_exclusive_lock(&lock_path(home), || {
            let path = store_path(home);
            let canonical = read_unlocked(&path)?;
            if !imported.is_empty() {
                write_unlocked(&path, &merge_maps(canonical, imported))?;
            }
            Ok(())
        })?;
        global.extra.remove(LEGACY_SETTINGS_KEY);
        settings::save(home, &global)
    })
}

fn load_home(home: &HomeLayout) -> io::Result<Vec<Value>> {
    migrate_settings_store(home)?;
    fs::with_exclusive_lock(&lock_path(home), || {
        Ok(read_unlocked(&store_path(home))?.into_values().collect())
    })
}

fn update<T>(
    state: &AppState,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, ApiError> {
    migrate_settings_store(&state.home).map_err(io_error)?;
    fs::with_exclusive_lock(&lock_path(&state.home), || {
        let path = store_path(&state.home);
        let mut connectors = read_unlocked(&path)?;
        let result = change(&mut connectors)?;
        write_unlocked(&path, &collapse_active_duplicates(connectors))?;
        Ok(result)
    })
    .map_err(io_error)
}

pub(super) fn load(state: &AppState) -> Result<Vec<Value>, ApiError> {
    load_home(&state.home).map_err(io_error)
}

pub(super) fn replace_active(state: &AppState, connector: &Value) -> Result<Vec<String>, ApiError> {
    update(state, |items| {
        let mut replaced = Vec::new();
        let now = cccc_contracts::utc_now();
        for item in items.values_mut() {
            let same = item["group_id"] == connector["group_id"]
                && item["actor_id"] == connector["actor_id"]
                && !item["revoked"].as_bool().unwrap_or(false);
            if !same {
                continue;
            }
            if let Some(id) = item["connector_id"].as_str() {
                replaced.push(id.to_owned());
            }
            item["revoked"] = Value::Bool(true);
            item["updated_at"] = json!(now);
        }
        let id = connector["connector_id"].as_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "connector_id is required")
        })?;
        let connector = normalized_entry(id, connector).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid web-model connector")
        })?;
        items.insert(id.to_owned(), connector);
        Ok(replaced)
    })
}

pub(super) fn revoke(state: &AppState, connector_id: &str) -> Result<bool, ApiError> {
    update(state, |items| {
        let Some(item) = items.get_mut(connector_id) else {
            return Ok(false);
        };
        item["revoked"] = Value::Bool(true);
        item["updated_at"] = json!(cccc_contracts::utc_now());
        Ok(true)
    })
}

pub(super) fn update_connector(
    state: &AppState,
    connector_id: &str,
    change: impl FnOnce(&mut Value),
) -> Result<bool, ApiError> {
    update(state, |items| {
        let Some(item) = items.get_mut(connector_id) else {
            return Ok(false);
        };
        change(item);
        Ok(true)
    })
}

fn secret_matches(item: &Value, supplied: &str) -> bool {
    item["secret"].as_str() == Some(supplied)
        || item["secret_hash"].as_str() == Some(hash_secret(supplied).as_str())
}

pub(super) fn find_authorized(
    state: &AppState,
    connector_id: &str,
    secret: Option<&str>,
) -> Result<Value, ApiError> {
    let item = load(state)?
        .into_iter()
        .find(|item| item["connector_id"] == connector_id)
        .ok_or_else(|| ApiError::not_found("web-model connector not found"))?;
    if item["revoked"].as_bool().unwrap_or(false) {
        return Err(ApiError::forbidden("web-model connector is revoked"));
    }
    if secret.is_some_and(|secret| !secret_matches(&item, secret)) {
        return Err(ApiError::forbidden("invalid web-model connector secret"));
    }
    let group_id = item["group_id"].as_str().unwrap_or("");
    let actor_id = item["actor_id"].as_str().unwrap_or("");
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(|_| ApiError::forbidden("web-model connector group is unavailable"))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| ApiError::forbidden("web-model connector actor is unavailable"))?;
    if actor.runtime != ActorRuntime::WebModel
        || actor.runner != RunnerKind::Headless
        || !actor.enabled
    {
        return Err(ApiError::forbidden(
            "web-model connector actor is stopped or no longer eligible",
        ));
    }
    Ok(item)
}

pub(super) fn for_actor(state: &AppState, group_id: &str, actor_id: &str) -> Option<Value> {
    load(state).ok()?.into_iter().find(|item| {
        item["group_id"] == group_id
            && item["actor_id"] == actor_id
            && !item["revoked"].as_bool().unwrap_or(false)
    })
}

pub(super) fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_settings_array_into_python_connector_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        integration_state::global_update(&home, LEGACY_SETTINGS_KEY, |value| {
            *value = json!([{
                "connector_id":"wmc_from_rust",
                "group_id":"g_test",
                "actor_id":"web1",
                "provider":"chatgpt_web",
                "secret":"wmcs_shared",
                "created_at":"2026-08-10T00:00:00Z",
                "updated_at":"2026-08-10T00:00:00Z",
                "revoked":false
            }]);
            Ok(())
        })
        .expect("legacy settings");

        let items = load_home(&home).expect("migrated connectors");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["connector_id"], "wmc_from_rust");
        assert!(secret_matches(&items[0], "wmcs_shared"));
        let settings =
            fs::read_yaml::<Value>(&home.root().join("settings.yaml")).expect("settings document");
        assert!(settings.get(LEGACY_SETTINGS_KEY).is_none());
        let canonical = fs::read_yaml::<Value>(&store_path(&home)).expect("canonical file");
        assert_eq!(
            canonical["connectors"]["wmc_from_rust"]["group_id"],
            "g_test"
        );
    }

    #[test]
    fn loads_python_hash_only_connector() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        fs::write_secret_yaml(
            &store_path(&home),
            &json!({"connectors":{"wmc_python":{
                "group_id":"g_test",
                "actor_id":"web1",
                "provider":"chatgpt_web",
                "secret_hash":hash_secret("wmcs_python"),
                "created_at":"2026-08-10T00:00:00Z",
                "updated_at":"2026-08-10T00:00:00Z",
                "revoked":false
            }}}),
        )
        .expect("python store");

        let items = load_home(&home).expect("load connector");

        assert_eq!(items.len(), 1);
        assert!(secret_matches(&items[0], "wmcs_python"));
        assert!(!secret_matches(&items[0], "wrong"));
    }

    #[test]
    fn legacy_json_settings_migrate_before_connector_store_locking() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        std::fs::write(
            home.root().join("settings.json"),
            serde_json::to_vec(&json!({
                LEGACY_SETTINGS_KEY: [{
                    "connector_id":"wmc_legacy_json",
                    "group_id":"g_test",
                    "actor_id":"web1",
                    "provider":"chatgpt_web",
                    "secret":"wmcs_legacy_json",
                    "created_at":"2026-08-10T00:00:00Z",
                    "updated_at":"2026-08-10T00:00:00Z",
                    "revoked":false
                }]
            }))
            .expect("legacy JSON"),
        )
        .expect("write legacy settings");

        let items = load_home(&home).expect("migrated connectors");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["connector_id"], "wmc_legacy_json");
        assert!(home.root().join(".rust-settings-migrated-v2").is_file());
    }
}

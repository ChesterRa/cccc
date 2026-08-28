use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{HomeLayout, fs, settings};

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
    let group_id = item.get("group_id")?.as_str()?.trim().to_owned();
    let actor_id = item.get("actor_id")?.as_str()?.trim().to_owned();
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
    item.insert("group_id".into(), json!(group_id));
    item.insert("actor_id".into(), json!(actor_id));
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
                let id = item["connector_id"]
                    .as_str()
                    .expect("normalized connector id")
                    .to_owned();
                result.insert(id, item);
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
            let id = item["connector_id"].as_str().unwrap_or(id).to_owned();
            result.insert(id, item);
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
    let retired_routes = canonical
        .values()
        .filter(|item| item["revoked"].as_bool().unwrap_or(false))
        .filter_map(|item| {
            let group_id = item["group_id"].as_str()?.trim();
            let actor_id = item["actor_id"].as_str()?.trim();
            (!group_id.is_empty() && !actor_id.is_empty())
                .then(|| (group_id.to_owned(), actor_id.to_owned()))
        })
        .collect::<std::collections::BTreeSet<_>>();
    for (connector_id, incoming) in imported {
        let route = (
            incoming["group_id"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_owned(),
            incoming["actor_id"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_owned(),
        );
        let Some(existing) = canonical.get(&connector_id) else {
            if !incoming["revoked"].as_bool().unwrap_or(false) && retired_routes.contains(&route) {
                continue;
            }
            canonical.insert(connector_id, incoming);
            continue;
        };
        let mut merged = incoming.as_object().cloned().unwrap_or_default();
        if let Some(existing) = existing.as_object() {
            merged.extend(existing.clone());
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
    if !settings::load(home)?
        .extra
        .contains_key(LEGACY_SETTINGS_KEY)
    {
        return Ok(());
    }
    settings::update(home, |global| {
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
        Ok(())
    })
}

fn update<T>(
    home: &HomeLayout,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> io::Result<T> {
    migrate_settings_store(home)?;
    fs::with_exclusive_lock(&lock_path(home), || {
        let path = store_path(home);
        let mut connectors = read_unlocked(&path)?;
        let result = change(&mut connectors)?;
        write_unlocked(&path, &collapse_active_duplicates(connectors))?;
        Ok(result)
    })
}

pub fn load(home: &HomeLayout) -> io::Result<Vec<Value>> {
    migrate_settings_store(home)?;
    fs::with_exclusive_lock(&lock_path(home), || {
        Ok(read_unlocked(&store_path(home))?.into_values().collect())
    })
}

pub fn replace_active(home: &HomeLayout, connector: &Value) -> io::Result<Vec<String>> {
    update(home, |items| {
        let id = connector["connector_id"].as_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "connector_id is required")
        })?;
        let connector = normalized_entry(id, connector).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid web-model connector")
        })?;
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
        let id = connector["connector_id"]
            .as_str()
            .expect("normalized connector id")
            .to_owned();
        items.insert(id, connector);
        Ok(replaced)
    })
}

pub fn revoke(home: &HomeLayout, connector_id: &str) -> io::Result<bool> {
    update(home, |items| {
        let Some(item) = items.get_mut(connector_id) else {
            return Ok(false);
        };
        item["revoked"] = Value::Bool(true);
        item["updated_at"] = json!(cccc_contracts::utc_now());
        Ok(true)
    })
}

pub fn retire_actor(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<Vec<Value>> {
    update(home, |items| {
        let mut retired = Vec::new();
        let now = cccc_contracts::utc_now();
        for item in items.values_mut() {
            if item["group_id"] != group_id
                || item["actor_id"] != actor_id
                || item["revoked"].as_bool().unwrap_or(false)
            {
                continue;
            }
            retired.push(item.clone());
            item["revoked"] = Value::Bool(true);
            item["updated_at"] = json!(now);
        }
        Ok(retired)
    })
}

pub fn retire_group(home: &HomeLayout, group_id: &str) -> io::Result<Vec<Value>> {
    update(home, |items| {
        let mut retired = Vec::new();
        let now = cccc_contracts::utc_now();
        for item in items.values_mut() {
            if item["group_id"] != group_id || item["revoked"].as_bool().unwrap_or(false) {
                continue;
            }
            retired.push(item.clone());
            item["revoked"] = Value::Bool(true);
            item["updated_at"] = json!(now);
        }
        Ok(retired)
    })
}

pub fn restore(home: &HomeLayout, entries: &[Value]) -> io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    update(home, |items| {
        for entry in entries {
            let id = entry["connector_id"].as_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "connector_id is required")
            })?;
            let normalized = normalized_entry(id, entry).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid web-model connector")
            })?;
            items.insert(id.to_owned(), normalized);
        }
        Ok(())
    })
}

pub fn update_connector(
    home: &HomeLayout,
    connector_id: &str,
    change: impl FnOnce(&mut Value),
) -> io::Result<bool> {
    update(home, |items| {
        let Some(item) = items.get_mut(connector_id) else {
            return Ok(false);
        };
        change(item);
        Ok(true)
    })
}

pub fn secret_matches(item: &Value, supplied: &str) -> bool {
    item["secret"].as_str() == Some(supplied)
        || item["secret_hash"].as_str() == Some(hash_secret(supplied).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_revocation_wins_over_a_newer_legacy_settings_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let connector = |revoked: bool, updated_at: &str| {
            json!({
                "connector_id":"wmc_retired",
                "group_id":"g_test",
                "actor_id":"web1",
                "provider":"chatgpt",
                "secret_hash":hash_secret("fixture-secret"),
                "revoked":revoked,
                "created_at":"2026-08-28T00:00:00Z",
                "updated_at":updated_at,
            })
        };
        write_unlocked(
            &store_path(&home),
            &Map::from_iter([(
                "wmc_retired".into(),
                connector(true, "2026-08-28T00:00:01Z"),
            )]),
        )
        .expect("canonical connector");
        settings::update(&home, |global| {
            global.extra.insert(
                LEGACY_SETTINGS_KEY.into(),
                json!({
                    "wmc_retired":connector(false, "2026-08-28T00:00:02Z"),
                    "wmc_retired_alias":{
                        "connector_id":" wmc_retired_alias ",
                        "group_id":" g_test ",
                        "actor_id":"web1 ",
                        "provider":"chatgpt",
                        "secret_hash":hash_secret("legacy-alias-secret"),
                        "revoked":false,
                        "created_at":"2026-08-28T00:00:02Z",
                        "updated_at":"2026-08-28T00:00:02Z",
                    }
                }),
            );
            Ok(())
        })
        .expect("legacy settings connector");

        let connectors = load(&home).expect("migrated connectors");
        let connector = connectors
            .iter()
            .find(|item| item["connector_id"] == "wmc_retired")
            .expect("retired connector");
        assert_eq!(connector["revoked"], true);
        assert_eq!(connectors.len(), 1);
        assert!(
            !settings::load(&home)
                .expect("settings")
                .extra
                .contains_key(LEGACY_SETTINGS_KEY)
        );
    }
}

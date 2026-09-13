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

/// Shared provisioning for the existing HTTP connector and Chat-first gateway.
pub fn create(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    provider: &str,
    label: &str,
) -> io::Result<(Value, Vec<String>)> {
    let group = crate::GroupStore::new(home.clone())?.load(group_id)?;
    if !crate::actors::find(&group, actor_id)
        .is_some_and(|actor| actor.runtime == cccc_contracts::ActorRuntime::WebModel)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "connector_actor_unavailable",
        ));
    }
    let now = cccc_contracts::utc_now();
    let connector = json!({
        "connector_id":format!("wmc_{}", &uuid::Uuid::new_v4().simple().to_string()[..16]),
        "kind":"web_model_connector","group_id":group_id,"actor_id":actor_id,
        "provider":if provider.trim().is_empty(){"chatgpt"}else{provider.trim()},"label":label,
        "secret":format!("wmcs_{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple()),
        "created_at":now,"updated_at":now,"revoked":false
    });
    let replaced = replace_active(home, &connector)?;
    Ok((connector, replaced))
}

/// A return target must be a stable ChatGPT conversation, not a provisional tab.
pub fn normalized_chatgpt_conversation_url(value: &str) -> Option<String> {
    let mut url = url::Url::parse(value.trim()).ok()?;
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if url.scheme() != "https"
        || !(host == "chatgpt.com" || host.ends_with(".chatgpt.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let parts = url.path_segments()?.collect::<Vec<_>>();
    let id = parts
        .windows(2)
        .find_map(|pair| (pair[0] == "c" && !pair[1].is_empty()).then_some(pair[1]))?;
    let upper = id.to_ascii_uppercase();
    if upper.starts_with("WEB:") || upper.starts_with("WEB%3A") {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

pub fn replace_active(home: &HomeLayout, connector: &Value) -> io::Result<Vec<String>> {
    let _dispatch = lock_browser_dispatch(home)?;
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
    let _dispatch = lock_browser_dispatch(home)?;
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
    let _dispatch = lock_browser_dispatch(home)?;
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
    let _dispatch = lock_browser_dispatch(home)?;
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
    let _dispatch = lock_browser_dispatch(home)?;
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

// Session routing reuses the connector store and its cross-process lock. The
// caller must obtain the session from the trusted transport, never tool arguments.
pub fn find_session(home: &HomeLayout, session: &str) -> io::Result<Option<Value>> {
    let result = find_binding_hash(home, "session_hash", session)?;
    if let Some(item) = &result {
        validate_session_actor(home, item)?;
    }
    Ok(result)
}

/// Locate a route only; `bind_session` revalidates the actor and consumes the code.
pub fn find_binding_code(home: &HomeLayout, code: &str) -> io::Result<Option<Value>> {
    find_binding_hash(home, "binding_code_hash", code)
}

fn find_binding_hash(home: &HomeLayout, field: &str, raw: &str) -> io::Result<Option<Value>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let wanted = hash_secret(raw);
    let mut matches = load(home)?.into_iter().filter(|item| {
        item["kind"] == "web_model_connector"
            && item["revoked"] != true
            && item[field].as_str() == Some(wanted.as_str())
    });
    let found = matches.next();
    if matches.next().is_some() {
        return Err(io::Error::other("connector_binding_conflict"));
    }
    Ok(found)
}

/// Issue a replacement code without changing the currently bound conversation.
/// Only the returned value contains the plaintext code; storage keeps its hash.
pub fn prepare_binding(
    home: &HomeLayout,
    connector_id: &str,
    ttl_seconds: i64,
) -> io::Result<Value> {
    let code = format!(
        "cccb_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let now = chrono::Utc::now();
    let expires = now + chrono::Duration::seconds(ttl_seconds.clamp(0, 3600));
    update(home, |items| {
        let item = items
            .get_mut(connector_id.trim())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "connector_not_found"))?;
        validate_session_actor(home, item)?;
        item["binding_code_hash"] = json!(hash_secret(&code));
        item["binding_expires_at"] = json!(expires.to_rfc3339());
        item["updated_at"] = json!(now.to_rfc3339());
        Ok(json!({"code":code,"binding_expires_at":expires.to_rfc3339()}))
    })
}

/// Consume a code and replace its session under the existing store lock.
/// This binds the inbound route only; it does not claim browser delivery works.
pub fn bind_session(
    home: &HomeLayout,
    connector_id: &str,
    code: &str,
    session: &str,
) -> io::Result<Value> {
    let _dispatch = lock_browser_dispatch(home)?;
    let session = session.trim();
    let code = code.trim();
    if session.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session_binding_required",
        ));
    }
    if code.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session_binding_code_invalid",
        ));
    }
    let connector_id = connector_id.trim();
    let session_hash = hash_secret(session);
    let code_hash = hash_secret(code);
    update(home, |items| {
        let item = items
            .get(connector_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "connector_not_found"))?;
        validate_session_actor(home, item)?;
        if item["binding_code_hash"].as_str() != Some(code_hash.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session_binding_code_invalid",
            ));
        }
        let now = chrono::Utc::now();
        let expires = item["binding_expires_at"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
        if expires.is_none_or(|value| value <= now) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "session_binding_code_expired",
            ));
        }
        if items.iter().any(|(id, other)| {
            id != connector_id
                && other["kind"] == "web_model_connector"
                && other["revoked"] != true
                && other["session_hash"].as_str() == Some(session_hash.as_str())
        }) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "session_already_bound",
            ));
        }
        let item = items
            .get_mut(connector_id)
            .expect("connector validated under lock");
        let old_hash = item["session_hash"].as_str().unwrap_or_default();
        if !old_hash.is_empty() && old_hash != session_hash {
            item["previous_session_hash"] = json!(old_hash);
        }
        item["session_hash"] = json!(session_hash);
        item["session_bound_at"] = json!(now.to_rfc3339());
        item["binding_code_hash"] = json!("");
        item["binding_expires_at"] = json!("");
        item["updated_at"] = json!(now.to_rfc3339());
        Ok(json!({
            "bound":true,
            "connector_id":connector_id,
            "group_id":item["group_id"],
            "actor_id":item["actor_id"]
        }))
    })
}

// The existing return target is tagged with its bound conversation. Rebinding
// changes one connector atomically; old target bytes remain but cannot be used.
fn target_matches_binding(connector: Option<&Value>, target: &Value) -> bool {
    let tagged = target["bound_session_hash"]
        .as_str()
        .filter(|s| !s.is_empty());
    match connector {
        Some(connector) => {
            let current = connector["session_hash"].as_str().filter(|s| !s.is_empty());
            if let Some(tagged) = tagged {
                return current == Some(tagged);
            }
            connector["previous_session_hash"]
                .as_str()
                .is_none_or(str::is_empty)
        }
        None => tagged.is_none(),
    }
}

/// Immutable ownership of one target and its current delivery attempt.
#[derive(Clone, PartialEq)]
pub struct BrowserTargetOwner {
    connector_id: Value,
    session_hash: Value,
    session_bound_at: Value,
    secret_hash: Value,
    target_id: Value,
    target_present: bool,
    delivery_id: Value,
}

impl BrowserTargetOwner {
    pub fn for_delivery(&self, delivery_id: &str) -> Self {
        Self {
            delivery_id: json!(delivery_id),
            ..self.clone()
        }
    }
}

fn target_snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    connectors: &Map<String, Value>,
) -> io::Result<(Value, BrowserTargetOwner)> {
    let connector = connectors.values().find(|item| {
        item["group_id"] == group_id && item["actor_id"] == actor_id && item["revoked"] != true
    });
    let store = crate::GroupStore::new(home.clone())?;
    let targets =
        crate::integration_state::group_get(&store, group_id, "web_model_browser_targets")?;
    let target = targets.get(actor_id).cloned().unwrap_or_else(|| json!({}));
    let disconnected = connector.is_none()
        && connectors
            .values()
            .any(|item| item["group_id"] == group_id && item["actor_id"] == actor_id);
    let target = if !disconnected && target_matches_binding(connector, &target) {
        target
    } else {
        json!({})
    };
    let connector = connector.unwrap_or(&Value::Null);
    let owner = BrowserTargetOwner {
        connector_id: connector["connector_id"].clone(),
        session_hash: connector["session_hash"].clone(),
        session_bound_at: connector["session_bound_at"].clone(),
        secret_hash: connector["secret_hash"].clone(),
        target_id: target["target_id"].clone(),
        target_present: target.as_object().is_some_and(|value| !value.is_empty()),
        delivery_id: target["last_delivery_id"].clone(),
    };
    Ok((target, owner))
}

pub fn browser_target_snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<(Value, BrowserTargetOwner)> {
    migrate_settings_store(home)?;
    fs::with_exclusive_lock(&lock_path(home), || {
        target_snapshot(home, group_id, actor_id, &read_unlocked(&store_path(home))?)
    })
}

pub fn browser_target(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<Value> {
    browser_target_snapshot(home, group_id, actor_id).map(|(target, _)| target)
}

fn lock_browser_dispatch(home: &HomeLayout) -> io::Result<std::fs::File> {
    fs::exclusive_lock(&home.root().join("state/web_model_dispatch.lock"))
}

/// Acquire on a blocking thread. Hold only across the final browser input, never
/// while calling the daemon, waiting for the page, or verifying its receipt.
pub fn browser_dispatch_permit(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    expected: &BrowserTargetOwner,
) -> io::Result<Option<std::fs::File>> {
    let permit = lock_browser_dispatch(home)?;
    let (target, owner) = browser_target_snapshot(home, group_id, actor_id)?;
    let group = crate::GroupStore::new(home.clone())?.load(group_id)?;
    let enabled = crate::actors::find(&group, actor_id).is_some_and(|actor| {
        actor.enabled && actor.runtime == cccc_contracts::ActorRuntime::WebModel
    });
    Ok(
        (enabled
            && &owner == expected
            && target.as_object().is_some_and(|value| !value.is_empty()))
        .then_some(permit),
    )
}

/// Compare ownership and patch under the same connector/group store locks.
/// A stale receipt remains a ledger fact, but cannot change the current target.
pub fn update_browser_target(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    expected: &BrowserTargetOwner,
    patch: &Map<String, Value>,
) -> io::Result<bool> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let connectors = read_unlocked(&store_path(home))?;
        let (_, owner) = target_snapshot(home, group_id, actor_id, &connectors)?;
        if &owner != expected {
            return Ok(false);
        }
        let store = crate::GroupStore::new(home.clone())?;
        crate::integration_state::group_update(
            &store,
            group_id,
            "web_model_browser_targets",
            |targets| {
                let Some(target) = targets.get_mut(actor_id).and_then(Value::as_object_mut) else {
                    return Ok(false);
                };
                target.extend(patch.clone());
                Ok(true)
            },
        )
    })
}

pub fn update_browser_connector(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    expected: &BrowserTargetOwner,
    change: impl FnOnce(&mut Value),
) -> io::Result<bool> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut connectors = read_unlocked(&store_path(home))?;
        let (_, owner) = target_snapshot(home, group_id, actor_id, &connectors)?;
        if &owner != expected {
            return Ok(false);
        }
        let Some(item) = expected
            .connector_id
            .as_str()
            .and_then(|id| connectors.get_mut(id))
        else {
            return Ok(false);
        };
        change(item);
        write_unlocked(&store_path(home), &connectors)?;
        Ok(true)
    })
}

/// Explicit target selection uses the same short lock as rebinding, avoiding a
/// split-file transaction or timestamps as a conversation-ownership signal.
pub fn save_browser_target(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    target: Option<Value>,
) -> io::Result<()> {
    let _dispatch = lock_browser_dispatch(home)?;
    migrate_settings_store(home)?;
    fs::with_exclusive_lock(&lock_path(home), || {
        let connectors = read_unlocked(&store_path(home))?;
        let connector = connectors.values().find(|item| {
            item["group_id"] == group_id && item["actor_id"] == actor_id && item["revoked"] != true
        });
        let mut target = target;
        if let Some(target) = target.as_mut() {
            if !target.is_object() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid browser target",
                ));
            }
            target["target_id"] = json!(uuid::Uuid::new_v4().to_string());
            target
                .as_object_mut()
                .expect("object checked")
                .remove("bound_session_hash");
            if let Some(hash) = connector
                .and_then(|item| item["session_hash"].as_str())
                .filter(|s| !s.is_empty())
            {
                target["bound_session_hash"] = json!(hash);
            }
        }
        let store = crate::GroupStore::new(home.clone())?;
        crate::integration_state::group_update(
            &store,
            group_id,
            "web_model_browser_targets",
            |targets| {
                if !targets.is_object() {
                    *targets = json!({});
                }
                let targets = targets.as_object_mut().expect("object initialized");
                if let Some(target) = target {
                    targets.insert(actor_id.to_owned(), target);
                } else {
                    targets.remove(actor_id);
                }
                Ok(())
            },
        )
    })
}

fn validate_session_actor(home: &HomeLayout, connector: &Value) -> io::Result<()> {
    if connector["kind"] != "web_model_connector" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid_connector_kind",
        ));
    }
    if connector["revoked"] == true {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "connector_revoked",
        ));
    }
    let group_id = connector["group_id"].as_str().unwrap_or_default();
    let actor_id = connector["actor_id"].as_str().unwrap_or_default();
    let group = crate::GroupStore::new(home.clone())?.load(group_id)?;
    let valid = crate::actors::find(&group, actor_id).is_some_and(|actor| {
        actor.enabled
            && actor.runtime == cccc_contracts::ActorRuntime::WebModel
            && crate::actors::effective_role(&group, actor_id).is_some()
    });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "connector_actor_unavailable",
        ));
    }
    Ok(())
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

    // Store-level fixtures intentionally bypass the daemon's separate singleton
    // policy. These tests do not claim multi-group browser support is enabled.
    fn session_fixture() -> (tempfile::TempDir, HomeLayout, Vec<String>) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = crate::GroupStore::new(home.clone()).expect("store");
        let mut groups = Vec::new();
        for id in ["route-a", "route-b"] {
            let mut group = store.create(id, "").expect("group");
            let mut actor = cccc_contracts::Actor::new("web-lead");
            actor.runtime = cccc_contracts::ActorRuntime::WebModel;
            crate::actors::add(&mut group, actor).expect("actor");
            store.save(&group).expect("save group");
            replace_active(
                &home,
                &json!({
                    "connector_id":id,"group_id":group.group_id,"actor_id":"web-lead",
                    "secret_hash":hash_secret("fixture-only"),"provider":"chatgpt"
                }),
            )
            .expect("connector");
            groups.push(group.group_id);
        }
        (temp, home, groups)
    }

    fn issue(home: &HomeLayout, connector: &str) -> String {
        prepare_binding(home, connector, 600).expect("prepare")["code"]
            .as_str()
            .expect("code")
            .to_owned()
    }

    #[test]
    fn session_binding_preserves_old_owner_until_single_use_replacement_commits() {
        let (_temp, home, groups) = session_fixture();
        let first = issue(&home, "route-a");
        assert_eq!(
            find_binding_code(&home, &first)
                .expect("fixture operation succeeds")
                .expect("fixture operation succeeds")["connector_id"],
            "route-a"
        );
        let bound = bind_session(&home, "route-a", &first, "chat-original")
            .expect("fixture operation succeeds");
        assert_eq!(bound["group_id"], groups[0]);
        assert!(bound.get("secret").is_none());
        let next = issue(&home, "route-a");
        assert!(
            find_session(&home, "chat-original")
                .expect("fixture operation succeeds")
                .is_some()
        );
        bind_session(&home, "route-a", &next, "chat-replacement")
            .expect("fixture operation succeeds");
        assert!(
            find_session(&home, "chat-original")
                .expect("fixture operation succeeds")
                .is_none()
        );
        assert!(
            find_binding_code(&home, &next)
                .expect("fixture operation succeeds")
                .is_none()
        );
        assert_eq!(
            bind_session(&home, "route-a", &next, "chat-attacker")
                .expect_err("binding must be rejected")
                .to_string(),
            "session_binding_code_invalid"
        );
        let restarted =
            HomeLayout::from_path(home.root().to_path_buf()).expect("fixture operation succeeds");
        assert_eq!(
            find_session(&restarted, "chat-replacement")
                .expect("fixture operation succeeds")
                .expect("fixture operation succeeds")["group_id"],
            groups[0]
        );
        let disk = std::fs::read_to_string(store_path(&home)).expect("fixture operation succeeds");
        for secret in [&first, &next, "chat-original", "chat-replacement"] {
            assert!(!disk.contains(secret), "raw binding material was persisted");
        }
    }

    #[test]
    fn binding_rejections_keep_the_current_owner_and_never_consume_the_other_code() {
        let (_temp, home, groups) = session_fixture();
        let a = issue(&home, "route-a");
        let b = issue(&home, "route-b");
        bind_session(&home, "route-a", &a, "chat-a").expect("bind route-a");
        // A rejected attempt never consumes the other code: the same `b`
        // still binds its own chat afterwards.
        assert_eq!(
            bind_session(&home, "route-b", &b, "chat-a")
                .expect_err("cross-group session reuse")
                .to_string(),
            "session_already_bound"
        );
        bind_session(&home, "route-b", &b, "chat-b").expect("bind route-b with the same code");
        assert_eq!(
            find_session(&home, "chat-a")
                .expect("route-a session")
                .expect("route-a session entry")["group_id"],
            groups[0]
        );
        assert_eq!(
            find_session(&home, "chat-b")
                .expect("route-b session")
                .expect("route-b session entry")["group_id"],
            groups[1]
        );
        assert!(
            find_session(&home, "")
                .expect("empty session lookup")
                .is_none()
        );
        assert!(
            find_binding_code(&home, "")
                .expect("empty code lookup")
                .is_none()
        );
        let expired = prepare_binding(&home, "route-a", 0).expect("expired prepare")["code"]
            .as_str()
            .expect("expired code")
            .to_owned();
        assert_eq!(
            bind_session(&home, "route-a", &expired, "chat-replacement")
                .expect_err("expired code")
                .to_string(),
            "session_binding_code_expired"
        );
        let superseded = issue(&home, "route-a");
        let current = issue(&home, "route-a");
        assert_eq!(
            bind_session(&home, "route-a", &superseded, "chat-replacement")
                .expect_err("superseded unused code")
                .to_string(),
            "session_binding_code_invalid"
        );
        assert_eq!(
            bind_session(&home, "route-a", "", "chat-replacement")
                .expect_err("empty code")
                .to_string(),
            "session_binding_code_invalid"
        );
        assert_eq!(
            bind_session(&home, "route-a", &current, " ")
                .expect_err("blank session")
                .to_string(),
            "session_binding_required"
        );
        assert!(
            find_session(&home, "chat-a")
                .expect("owner preserved through every rejection")
                .is_some()
        );
        // Revocation is terminal for this connector, so it runs last.
        revoke(&home, "route-a").expect("revoke route-a");
        assert!(
            find_session(&home, "chat-a")
                .expect("revoked session lookup")
                .is_none()
        );
        assert_eq!(
            bind_session(&home, "route-a", &current, "chat-replacement")
                .expect_err("revoked connector")
                .to_string(),
            "connector_revoked"
        );
    }

    #[test]
    fn session_binding_revalidates_enabled_web_members_without_freezing_their_role() {
        let (_temp, home, groups) = session_fixture();
        let store = crate::GroupStore::new(home.clone()).expect("fixture operation succeeds");
        let original = store.load(&groups[0]).expect("fixture operation succeeds");
        bind_session(&home, "route-a", &issue(&home, "route-a"), "active-chat")
            .expect("fixture operation succeeds");
        let pending = issue(&home, "route-a");
        for change in ["disabled", "local", "removed"] {
            let mut group = original.clone();
            match change {
                "disabled" => group.actors[0].enabled = false,
                "local" => group.actors[0].runtime = cccc_contracts::ActorRuntime::Codex,
                "removed" => group.actors.clear(),
                _ => unreachable!(),
            }
            store.save(&group).expect("fixture operation succeeds");
            assert_eq!(
                bind_session(&home, "route-a", &pending, "new-chat")
                    .expect_err("binding must be rejected")
                    .to_string(),
                "connector_actor_unavailable"
            );
            assert!(find_session(&home, "active-chat").is_err());
            assert!(prepare_binding(&home, "route-a", 600).is_err());
        }
        let mut demoted = original;
        demoted
            .actors
            .insert(0, cccc_contracts::Actor::new("new-lead"));
        store.save(&demoted).expect("fixture operation succeeds");
        assert!(
            find_session(&home, "active-chat")
                .expect("role can change")
                .is_some()
        );
        bind_session(&home, "route-a", &pending, "new-chat").expect("peer can bind");
        assert!(
            find_session(&home, "new-chat")
                .expect("fixture operation succeeds")
                .is_some()
        );
    }

    #[test]
    fn session_binding_parallel_claims_have_exactly_one_winner() {
        for same_code in [true, false] {
            let (_temp, home, _) = session_fixture();
            let a = issue(&home, "route-a");
            let b = if same_code {
                a.clone()
            } else {
                issue(&home, "route-b")
            };
            let barrier = std::sync::Barrier::new(2);
            let results = std::thread::scope(|scope| {
                let first = scope.spawn(|| {
                    barrier.wait();
                    bind_session(&home, "route-a", &a, "chat-one")
                });
                let second = scope.spawn(|| {
                    barrier.wait();
                    bind_session(
                        &home,
                        if same_code { "route-a" } else { "route-b" },
                        &b,
                        if same_code { "chat-two" } else { "chat-one" },
                    )
                });
                [
                    first.join().expect("fixture operation succeeds"),
                    second.join().expect("fixture operation succeeds"),
                ]
            });
            assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
            let error = results
                .into_iter()
                .find_map(Result::err)
                .expect("fixture operation succeeds");
            assert_eq!(
                error.to_string(),
                if same_code {
                    "session_binding_code_invalid"
                } else {
                    "session_already_bound"
                }
            );
        }
    }

    // Shared by our two cross-process lock tests; always reap the owned child.
    fn wait_for_test_child(mut child: std::process::Child) -> std::process::Output {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().expect("child completion status").is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill(); // The child may have exited since try_wait.
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().expect("reap test child")
    }

    #[test]
    fn session_binding_respects_the_existing_lock_across_processes() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        const CHILD_HOME: &str = "CCCC_TEST_SESSION_BINDING_CHILD_HOME";
        if let Some(path) = std::env::var_os(CHILD_HOME) {
            let home = HomeLayout::from_path(PathBuf::from(path)).expect("child home");
            let code = std::env::var("CCCC_TEST_SESSION_BINDING_CODE").expect("fixture code");
            std::fs::write(home.root().join("child-started"), "ready").expect("signal started");
            bind_session(&home, "route-a", &code, "child-chat").expect("child binding");
            std::fs::write(home.root().join("child-bound"), "bound").expect("signal bound");
            return;
        }
        let (_temp, home, _) = session_fixture();
        let code = issue(&home, "route-a");
        let (child, blocked) = fs::with_exclusive_lock(&lock_path(&home), || {
            let mut child = Command::new(std::env::current_exe()?)
                .args(["--exact", "web_model_connectors::tests::session_binding_respects_the_existing_lock_across_processes"])
                .env(CHILD_HOME, home.root())
                .env("CCCC_TEST_SESSION_BINDING_CODE", &code)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while !home.root().join("child-started").exists()
                && child.try_wait()?.is_none() && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(200));
            let blocked = home.root().join("child-started").exists()
                && !home.root().join("child-bound").exists()
                && child.try_wait()?.is_none();
            Ok((child, blocked))
        }).expect("hold existing store lock");
        let status = wait_for_test_child(child).status;
        assert!(
            blocked,
            "another process bypassed the existing connector lock"
        );
        assert!(
            status.success(),
            "child could not bind after releasing the lock"
        );
        assert!(
            find_session(&home, "child-chat")
                .expect("read child binding")
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_binding_write_failure_does_not_consume_code_or_replace_owner() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, home, _) = session_fixture();
        bind_session(&home, "route-a", &issue(&home, "route-a"), "old-chat")
            .expect("fixture operation succeeds");
        let pending = issue(&home, "route-a");
        std::fs::set_permissions(home.root(), std::fs::Permissions::from_mode(0o500))
            .expect("fixture operation succeeds");
        let failed = bind_session(&home, "route-a", &pending, "new-chat");
        std::fs::set_permissions(home.root(), std::fs::Permissions::from_mode(0o700))
            .expect("fixture operation succeeds");
        assert_eq!(
            failed.expect_err("binding must be rejected").kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(
            find_session(&home, "old-chat")
                .expect("fixture operation succeeds")
                .is_some()
        );
        assert!(
            find_session(&home, "new-chat")
                .expect("fixture operation succeeds")
                .is_none()
        );
        bind_session(&home, "route-a", &pending, "new-chat").expect("fixture operation succeeds");
    }
    #[test]
    fn replacement_binding_never_sends_new_chats_reports_to_the_previous_chat() {
        let (_temp, home, groups) = session_fixture();
        let store = crate::GroupStore::new(home.clone()).expect("store");
        bind_session(&home, "route-a", &issue(&home, "route-a"), "old-chat").expect("old binding");
        crate::integration_state::group_update(&store,&groups[0],"web_model_browser_targets",|targets|{
            *targets=json!({"web-lead":{"kind":"existing_chat","url":"https://chatgpt.com/c/old-chat"}});Ok(())
        }).expect("old callback");
        let replacement = issue(&home, "route-a");
        assert!(bind_session(&home, "route-a", "wrong-code", "new-chat").is_err());
        assert_eq!(
            store.load(&groups[0]).expect("group").extra["web_model_browser_targets"]["web-lead"]["url"],
            "https://chatgpt.com/c/old-chat"
        );
        bind_session(&home, "route-a", &replacement, "new-chat").expect("replace binding");
        let group = store.load(&groups[0]).expect("group");
        assert_eq!(
            group.extra["web_model_browser_targets"]["web-lead"]["url"],
            "https://chatgpt.com/c/old-chat"
        );
        assert!(
            browser_target(&home, &groups[0], "web-lead")
                .expect("effective target")
                .get("url")
                .is_none(),
            "new Chat can use the old Chat's callback"
        );
        save_browser_target(
            &home,
            &groups[0],
            "web-lead",
            Some(json!({"kind":"existing_chat","url":"https://chatgpt.com/c/new-chat"})),
        )
        .expect("select new callback");
        assert_eq!(
            browser_target(&home, &groups[0], "web-lead").expect("target")["url"],
            "https://chatgpt.com/c/new-chat"
        );
        bind_session(&home, "route-a", &issue(&home, "route-a"), "new-chat")
            .expect("same-owner rebind");
        assert_eq!(
            browser_target(&home, &groups[0], "web-lead").expect("target")["url"],
            "https://chatgpt.com/c/new-chat"
        );
        assert!(
            find_session(&home, "old-chat")
                .expect("old lookup")
                .is_none()
        );
        assert!(
            find_session(&home, "new-chat")
                .expect("new lookup")
                .is_some()
        );
    }
    fn replace_delivery_route(home: &HomeLayout, group_id: &str, action: &str) {
        match action {
            "bind" => {
                bind_session(home, "route-a", &issue(home, "route-a"), "new-chat").expect("rebind");
            }
            "revoke" => {
                assert!(revoke(home, "route-a").expect("revoke"));
            }
            "target" => {
                save_browser_target(
                    home,
                    group_id,
                    "web-lead",
                    Some(json!({"kind":"existing_chat","url":"https://chatgpt.com/c/old"})),
                )
                .expect("replace same URL");
            }
            "clear" => {
                save_browser_target(home, group_id, "web-lead", None).expect("clear target");
            }
            "rotate" => {
                create(home, group_id, "web-lead", "chatgpt", "replacement")
                    .expect("replace connector");
            }
            "retire_actor" => {
                retire_actor(home, group_id, "web-lead").expect("retire actor");
            }
            "retire_group" => {
                retire_group(home, group_id).expect("retire group");
            }
            _ => panic!("unknown route change"),
        }
    }

    #[test]
    fn obsolete_browser_attempts_cannot_write_any_part_of_the_replacement() {
        for action in [
            "bind",
            "revoke",
            "target",
            "clear",
            "rotate",
            "retire_actor",
            "retire_group",
            "new_attempt",
        ] {
            let (_temp, home, groups) = session_fixture();
            let group_id = &groups[0];
            bind_session(&home, "route-a", &issue(&home, "route-a"), "old-chat")
                .expect("old binding");
            save_browser_target(&home, group_id, "web-lead", Some(json!({
                "kind":"existing_chat","url":"https://chatgpt.com/c/old","last_delivery_id":"old-attempt"
            }))).expect("old target");
            let (_, owner) = browser_target_snapshot(&home, group_id, "web-lead").expect("owner");
            if action == "new_attempt" {
                assert!(
                    update_browser_target(
                        &home,
                        group_id,
                        "web-lead",
                        &owner,
                        json!({"last_delivery_id":"new-attempt"})
                            .as_object()
                            .expect("F1 connector fixture value")
                    )
                    .expect("new attempt")
                );
            } else {
                replace_delivery_route(&home, group_id, action);
            }
            let store = crate::GroupStore::new(home.clone()).expect("store");
            let before_target =
                crate::integration_state::group_get(&store, group_id, "web_model_browser_targets")
                    .expect("target before late response");
            let before_connectors = load(&home).expect("connectors before late response");
            // Ownership is checked before either payload is inspected; repeated status strings add no branch.
            assert!(!update_browser_target(&home, group_id, "web-lead", &owner,
                json!({"last_delivery_status":"submitted", "url":"https://chatgpt.com/c/late-old-url"})
                    .as_object().expect("late patch")).expect("conditional patch"), "{action}");
            assert!(
                !update_browser_connector(
                    &home,
                    group_id,
                    "web-lead",
                    &owner,
                    |item| item["last_call_status"] = json!("submitted")
                )
                .expect("conditional activity"),
                "{action}"
            );
            assert_eq!(
                crate::integration_state::group_get(&store, group_id, "web_model_browser_targets")
                    .expect("target after"),
                before_target,
                "{action}"
            );
            assert_eq!(
                load(&home).expect("connectors after"),
                before_connectors,
                "{action}"
            );
            assert!(
                browser_dispatch_permit(&home, group_id, "web-lead", &owner)
                    .expect("obsolete permit")
                    .is_none(),
                "{action}"
            );
        }
    }

    #[test]
    fn dispatch_permit_orders_route_changes_across_processes_without_locking_reads() {
        use fs2::FileExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        const CHILD_HOME: &str = "CCCC_TEST_BROWSER_DISPATCH_HOME";
        if let Some(path) = std::env::var_os(CHILD_HOME) {
            let home = HomeLayout::from_path(PathBuf::from(path)).expect("child home");
            let group_id = std::env::var("CCCC_TEST_BROWSER_DISPATCH_GROUP").expect("group");
            let action = std::env::var("CCCC_TEST_BROWSER_DISPATCH_ACTION").expect("action");
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(home.root().join("state/web_model_dispatch.lock"))
                .expect("shared gate");
            assert_eq!(
                file.try_lock_exclusive()
                    .expect_err("parent owns Send")
                    .kind(),
                fs2::lock_contended_error().kind()
            );
            std::fs::write(home.root().join("gate-observed"), "locked").expect("signal gate");
            replace_delivery_route(&home, &group_id, &action);
            return;
        }
        for action in [
            "bind",
            "revoke",
            "target",
            "clear",
            "rotate",
            "retire_actor",
            "retire_group",
        ] {
            let (_temp, home, groups) = session_fixture();
            bind_session(&home, "route-a", &issue(&home, "route-a"), "old-chat").expect("binding");
            save_browser_target(
                &home,
                &groups[0],
                "web-lead",
                Some(json!({"kind":"existing_chat","url":"https://chatgpt.com/c/old"})),
            )
            .expect("target");
            let (target, owner) =
                browser_target_snapshot(&home, &groups[0], "web-lead").expect("snapshot");
            let permit = browser_dispatch_permit(&home, &groups[0], "web-lead", &owner)
                .expect("Send gate")
                .expect("current owner");
            let mut child = Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", "web_model_connectors::tests::dispatch_permit_orders_route_changes_across_processes_without_locking_reads", "--nocapture"])
                .env(CHILD_HOME, home.root()).env("CCCC_TEST_BROWSER_DISPATCH_GROUP", &groups[0])
                .env("CCCC_TEST_BROWSER_DISPATCH_ACTION", action)
                .stdout(Stdio::null()).stderr(Stdio::piped()).spawn().expect("child writer");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !home.root().join("gate-observed").exists()
                && child.try_wait().expect("child status").is_none()
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            let blocked = home.root().join("gate-observed").exists()
                && child.try_wait().expect("writer blocked").is_none();
            // Reads and unrelated group work do not wait for the final browser input.
            assert_eq!(
                browser_target(&home, &groups[0], "web-lead").expect("read during Send"),
                target
            );
            let store = crate::GroupStore::new(home.clone()).expect("store");
            store
                .mutate(&groups[1], |group| {
                    group.title = "other group remains available".into();
                    Ok(())
                })
                .expect("other group write");
            drop(permit);
            let output = wait_for_test_child(child);
            assert!(blocked, "{action}: writer bypassed Send gate");
            assert!(
                output.status.success(),
                "{action}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                browser_dispatch_permit(&home, &groups[0], "web-lead", &owner)
                    .expect("old permission")
                    .is_none(),
                "{action}: old owner survived"
            );
        }
    }

    #[test]
    fn revoking_an_unbound_connector_also_invalidates_its_browser_target() {
        let (_temp, home, groups) = session_fixture();
        save_browser_target(
            &home,
            &groups[0],
            "web-lead",
            Some(json!({"kind":"existing_chat","url":"https://chatgpt.com/c/unbound"})),
        )
        .expect("unbound target");
        assert!(revoke(&home, "route-a").expect("revoke"));
        assert_eq!(
            browser_target(&home, &groups[0], "web-lead").expect("disconnected target"),
            json!({})
        );
    }
    #[test]
    fn retained_targets_without_an_id_allow_receipts_but_not_writes_after_clear() {
        let (_temp, home, groups) = session_fixture();
        let store = crate::GroupStore::new(home.clone()).expect("store");
        crate::integration_state::group_update(&store, &groups[0], "web_model_browser_targets", |targets| {
            *targets = json!({"web-lead":{"kind":"new_chat","url":"https://chatgpt.com/","last_delivery_id":"retained-attempt"}});
            Ok(())
        }).expect("retained target");
        let (_, owner) =
            browser_target_snapshot(&home, &groups[0], "web-lead").expect("retained owner");
        assert!(
            update_browser_target(
                &home,
                &groups[0],
                "web-lead",
                &owner,
                json!({"kind":"existing_chat","url":"https://chatgpt.com/c/resolved"})
                    .as_object()
                    .expect("F1 connector fixture value")
            )
            .expect("late URL for same target")
        );
        assert!(
            update_browser_target(
                &home,
                &groups[0],
                "web-lead",
                &owner,
                json!({"last_delivery_status":"submitted"})
                    .as_object()
                    .expect("F1 connector fixture value")
            )
            .expect("same attempt final receipt")
        );
        save_browser_target(&home, &groups[0], "web-lead", None).expect("clear");
        assert!(
            !update_browser_target(
                &home,
                &groups[0],
                "web-lead",
                &owner,
                json!({"last_delivery_status":"failed"})
                    .as_object()
                    .expect("F1 connector fixture value")
            )
            .expect("obsolete receipt")
        );
        assert!(
            !update_browser_connector(
                &home,
                &groups[0],
                "web-lead",
                &owner,
                |item| item["last_call_status"] = json!("failed")
            )
            .expect("obsolete activity")
        );
    }
}

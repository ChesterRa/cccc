use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::HomeLayout;
use crate::capabilities::Capability;

#[derive(Default)]
pub struct LegacyCapabilityScope {
    pub enabled: BTreeSet<String>,
    pub blocked: BTreeSet<String>,
    pub hidden: BTreeSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CatalogFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

struct CachedCatalog {
    fingerprint: CatalogFingerprint,
    capabilities: Vec<Capability>,
}

fn catalog_cache() -> &'static Mutex<HashMap<PathBuf, CachedCatalog>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedCatalog>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn catalog(home: &HomeLayout) -> io::Result<Vec<Capability>> {
    let path = capability_state_path(home, "catalog.json");
    let Some(fingerprint) = fingerprint(&path)? else {
        return Ok(Vec::new());
    };
    if let Some(capabilities) = cached_catalog(&path, fingerprint) {
        return Ok(capabilities);
    }
    let value = crate::fs::read_json::<Value>(&path)?;
    let capabilities = record_values(value.get("records"))
        .filter_map(parse_capability)
        .collect::<Vec<_>>();
    catalog_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            path,
            CachedCatalog {
                fingerprint,
                capabilities: capabilities.clone(),
            },
        );
    Ok(capabilities)
}

fn fingerprint(path: &Path) -> io::Result<Option<CatalogFingerprint>> {
    match path.metadata() {
        Ok(metadata) => Ok(Some(CatalogFingerprint {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn cached_catalog(path: &Path, fingerprint: CatalogFingerprint) -> Option<Vec<Capability>> {
    catalog_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
        .filter(|cached| cached.fingerprint == fingerprint)
        .map(|cached| cached.capabilities.clone())
}

fn record_values(value: Option<&Value>) -> Box<dyn Iterator<Item = &Value> + '_> {
    match value {
        Some(Value::Array(records)) => Box::new(records.iter()),
        Some(Value::Object(records)) => Box::new(records.values()),
        _ => Box::new(std::iter::empty()),
    }
}

pub fn scope(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<LegacyCapabilityScope> {
    let value = read_json(home, "state.json")?;
    let mut scope = LegacyCapabilityScope::default();
    collect_strings(
        nested(&value, &["group_enabled", group_id]),
        &mut scope.enabled,
    );
    collect_strings(
        nested(&value, &["actor_enabled", group_id, actor_id]),
        &mut scope.enabled,
    );
    collect_session(
        nested(&value, &["session_enabled", group_id, actor_id]),
        &mut scope.enabled,
    );
    collect_blocks(value.get("global_blocked"), &mut scope.blocked);
    collect_blocks(
        nested(&value, &["group_blocked", group_id]),
        &mut scope.blocked,
    );
    collect_strings(
        nested(&value, &["actor_hidden", group_id, actor_id]),
        &mut scope.hidden,
    );
    Ok(scope)
}

fn parse_capability(value: &Value) -> Option<Capability> {
    let id = nonempty(value.get("capability_id").or_else(|| value.get("id")))?;
    let name = nonempty(value.get("name")).unwrap_or(id);
    Some(Capability {
        id: id.to_owned(),
        kind: nonempty(value.get("kind")).unwrap_or_default().to_owned(),
        name: name.to_owned(),
        description: nonempty(
            value
                .get("description_short")
                .or_else(|| value.get("description")),
        )
        .unwrap_or("")
        .to_owned(),
        tool_names: strings(value.get("tool_names")),
        tags: strings(value.get("tags")),
        capsule_text: value
            .get("capsule_text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        source: nonempty(value.get("source_id"))
            .unwrap_or("legacy")
            .to_owned(),
        source_uri: nonempty(value.get("source_uri"))
            .unwrap_or_default()
            .to_owned(),
        qualification_status: nonempty(value.get("qualification_status"))
            .unwrap_or("qualified")
            .to_owned(),
        enable_supported: value
            .get("enable_supported")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn collect_strings(value: Option<&Value>, output: &mut BTreeSet<String>) {
    output.extend(strings(value));
}

fn collect_blocks(value: Option<&Value>, output: &mut BTreeSet<String>) {
    match value {
        Some(Value::Object(entries)) => output.extend(
            entries
                .iter()
                .filter(|(_, entry)| crate::capabilities::block_entry_is_active(entry))
                .map(|(id, _)| id.clone()),
        ),
        _ => collect_strings(value, output),
    }
}

fn collect_session(value: Option<&Value>, output: &mut BTreeSet<String>) {
    let now = Utc::now();
    for item in value.and_then(Value::as_array).into_iter().flatten() {
        let active = item
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|expires_at| expires_at > now);
        if active && let Some(id) = nonempty(item.get("capability_id")) {
            output.insert(id.to_owned());
        }
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::Object(items)) => items.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn nonempty(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn nested<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn read_json(home: &HomeLayout, name: &str) -> io::Result<Value> {
    let path = capability_state_path(home, name);
    if !path.exists() {
        return Ok(Value::Null);
    }
    crate::fs::read_json(&path)
}

fn capability_state_path(home: &HomeLayout, name: &str) -> PathBuf {
    home.root().join("state/capabilities").join(name)
}

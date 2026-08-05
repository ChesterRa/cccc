use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;

use crate::HomeLayout;
use crate::capability_builtin;
use crate::fs::{read_json, with_exclusive_lock, write_json};
use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};

mod state;
pub use state::CapabilityState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capsule_text: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_uri: String,
}

#[derive(Debug, Clone)]
pub struct CapabilityStore {
    home: HomeLayout,
}

impl CapabilityStore {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        Self { home }
    }

    pub fn load(&self) -> io::Result<CapabilityState> {
        self.migrate_legacy()?;
        let path = self.path();
        if path.exists() {
            let raw: Value = read_json(&path)?;
            let mut state = CapabilityState::default();
            state.blocked.extend(
                raw.get("global_blocked")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .map(|(id, _)| id.clone()),
            );
            Ok(state)
        } else {
            Ok(CapabilityState::default())
        }
    }

    pub fn save(&self, state: &CapabilityState) -> io::Result<()> {
        self.mutate_state(|raw| {
            let blocked = object_field(raw, "global_blocked");
            blocked.clear();
            for id in &state.blocked {
                blocked.insert(
                    id.clone(),
                    json!({"reason":"","by":"user","blocked_at":utc_now(),"expires_at":""}),
                );
            }
            Ok(())
        })
    }

    pub fn catalog(&self) -> io::Result<Vec<Capability>> {
        self.migrate_legacy()?;
        let mut items = BTreeMap::new();
        for capability in capability_builtin::all()
            .into_iter()
            .chain(crate::capability_legacy::catalog(&self.home)?)
            .chain(self.load()?.custom.into_values())
        {
            items.insert(capability.id.clone(), capability);
        }
        Ok(items.into_values().collect())
    }

    pub fn search(&self, query: &str) -> io::Result<Vec<Capability>> {
        let terms: Vec<_> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        Ok(self
            .catalog()?
            .into_iter()
            .filter(|item| {
                if terms.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {}",
                    item.id,
                    item.name,
                    item.description,
                    item.tags.join(" ")
                )
                .to_lowercase();
                terms.iter().all(|term| haystack.contains(term))
            })
            .collect())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> io::Result<CapabilityState> {
        self.set_enabled_for(id, enabled, "", "", "group", 3600)
    }

    pub fn set_blocked(&self, id: &str, blocked: bool) -> io::Result<CapabilityState> {
        self.set_blocked_for(id, blocked, "", "", "user", 0)
    }

    pub fn set_hidden(&self, id: &str, hidden: bool) -> io::Result<CapabilityState> {
        self.set_hidden_for(id, hidden, "", "")
    }

    pub fn import(&self, capability: Capability) -> io::Result<CapabilityState> {
        validate_id(&capability.id)?;
        let path = self.catalog_path();
        with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut raw = if path.exists() {
                read_json::<Value>(&path)?
            } else {
                json!({"v":1,"created_at":utc_now(),"sources":{},"records":{}})
            };
            raw["updated_at"] = json!(utc_now());
            object_field(&mut raw, "records").insert(
                capability.id.clone(),
                json!({
                    "capability_id":capability.id,
                    "kind":capability.kind,
                    "name":capability.name,
                    "description_short":capability.description,
                    "tool_names":capability.tool_names,
                    "tags":capability.tags,
                    "capsule_text":capability.capsule_text,
                    "source_id":if capability.source.is_empty(){"manual_import"}else{&capability.source},
                    "source_uri":capability.source_uri,
                    "qualification_status":"qualified",
                    "enable_supported":true
                }),
            );
            write_json(&path, &raw)
        })?;
        self.load()
    }

    pub fn uninstall(&self, id: &str) -> io::Result<bool> {
        let path = self.catalog_path();
        let removed = with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut raw = if path.exists() {
                read_json::<Value>(&path)?
            } else {
                json!({})
            };
            let removed = object_field(&mut raw, "records").remove(id).is_some();
            raw["updated_at"] = json!(utc_now());
            write_json(&path, &raw)?;
            Ok(removed)
        })?;
        self.remove_bindings(id)?;
        Ok(removed)
    }

    pub fn delete_source(&self, source: &str) -> io::Result<Vec<String>> {
        let removed = self
            .catalog()?
            .into_iter()
            .filter(|capability| capability.source == source)
            .map(|capability| capability.id)
            .collect::<Vec<_>>();
        for id in &removed {
            self.uninstall(id)?;
        }
        Ok(removed)
    }

    pub fn require(&self, id: &str) -> io::Result<Capability> {
        self.catalog()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("capability not found: {id}"),
                )
            })
    }

    pub fn catalog_record(&self, id: &str) -> io::Result<Option<Value>> {
        self.migrate_legacy()?;
        let path = self.catalog_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw: Value = read_json(&path)?;
        Ok(match raw.get("records") {
            Some(Value::Object(records)) => records.get(id).cloned(),
            Some(Value::Array(records)) => records
                .iter()
                .find(|record| {
                    record
                        .get("capability_id")
                        .or_else(|| record.get("id"))
                        .and_then(Value::as_str)
                        == Some(id)
                })
                .cloned(),
            _ => None,
        })
    }

    pub fn set_enabled_for(
        &self,
        id: &str,
        enabled: bool,
        group_id: &str,
        actor_id: &str,
        scope: &str,
        ttl_seconds: i64,
    ) -> io::Result<CapabilityState> {
        self.require(id)?;
        if group_id.is_empty() {
            return Err(io::Error::other("group_id is required"));
        }
        self.mutate_state(|raw| {
            match scope {
                "group" => {
                    let groups = object_field(raw, "group_enabled");
                    let items = groups.entry(group_id).or_insert_with(|| json!([]));
                    set_array_member(items, id, enabled);
                    remove_empty_entry(groups, group_id);
                }
                "actor" => {
                    if actor_id.is_empty() {
                        return Err(io::Error::other("actor_id is required for actor scope"));
                    }
                    let groups = object_field(raw, "actor_enabled");
                    let group = ensure_object(groups.entry(group_id).or_insert_with(|| json!({})));
                    let items = group.entry(actor_id).or_insert_with(|| json!([]));
                    set_array_member(items, id, enabled);
                    remove_empty_entry(group, actor_id);
                    remove_empty_entry(groups, group_id);
                }
                "session" => {
                    if actor_id.is_empty() {
                        return Err(io::Error::other("actor_id is required for session scope"));
                    }
                    let groups = object_field(raw, "session_enabled");
                    let group = ensure_object(groups.entry(group_id).or_insert_with(|| json!({})));
                    let items = group.entry(actor_id).or_insert_with(|| json!([]));
                    if !items.is_array() {
                        *items = json!([]);
                    }
                    let items = items.as_array_mut().expect("session list initialized");
                    items.retain(|item| item["capability_id"].as_str() != Some(id));
                    if enabled {
                        let ttl_seconds = ttl_seconds.clamp(60, 24 * 3600);
                        let expires_at =
                            chrono::Utc::now() + chrono::Duration::seconds(ttl_seconds);
                        items.push(json!({
                            "capability_id":id,
                            "expires_at":expires_at.to_rfc3339_opts(
                                chrono::SecondsFormat::Micros,
                                true,
                            ),
                        }));
                    }
                    remove_empty_entry(group, actor_id);
                    remove_empty_entry(groups, group_id);
                }
                _ => {
                    return Err(io::Error::other("scope must be group, actor, or session"));
                }
            }
            Ok(())
        })?;
        self.load()
    }

    pub fn set_blocked_for(
        &self,
        id: &str,
        blocked: bool,
        group_id: &str,
        reason: &str,
        by: &str,
        ttl_seconds: i64,
    ) -> io::Result<CapabilityState> {
        self.require(id)?;
        self.mutate_state(|raw| {
            let target = if group_id.is_empty() {
                object_field(raw, "global_blocked")
            } else {
                let groups = object_field(raw, "group_blocked");
                ensure_object(groups.entry(group_id).or_insert_with(|| json!({})))
            };
            if blocked {
                let expires_at = if ttl_seconds > 0 {
                    (chrono::Utc::now()
                        + chrono::Duration::seconds(ttl_seconds.clamp(1, 30 * 24 * 3600)))
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
                } else {
                    String::new()
                };
                target.insert(
                    id.into(),
                    json!({
                        "reason":reason.chars().take(280).collect::<String>(),
                        "by":by,
                        "blocked_at":utc_now(),
                        "expires_at":expires_at,
                    }),
                );
            } else {
                target.remove(id);
            }
            Ok(())
        })?;
        self.load()
    }

    pub fn set_hidden_for(
        &self,
        id: &str,
        hidden: bool,
        group_id: &str,
        actor_id: &str,
    ) -> io::Result<CapabilityState> {
        self.require(id)?;
        if group_id.is_empty() || actor_id.is_empty() {
            return Err(io::Error::other("group_id and actor_id are required"));
        }
        self.mutate_state(|raw| {
            let groups = object_field(raw, "actor_hidden");
            let group = ensure_object(groups.entry(group_id).or_insert_with(|| json!({})));
            let items = group.entry(actor_id).or_insert_with(|| json!([]));
            set_array_member(items, id, hidden);
            remove_empty_entry(group, actor_id);
            remove_empty_entry(groups, group_id);
            Ok(())
        })?;
        self.load()
    }

    fn mutate_state<T>(&self, change: impl FnOnce(&mut Value) -> io::Result<T>) -> io::Result<T> {
        let path = self.path();
        with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut raw = if path.exists() {
                read_json::<Value>(&path)?
            } else {
                json!({"v":1,"created_at":utc_now()})
            };
            let result = change(&mut raw)?;
            raw["v"] = json!(1);
            raw["updated_at"] = json!(utc_now());
            write_json(&path, &raw)?;
            Ok(result)
        })
    }

    fn remove_bindings(&self, id: &str) -> io::Result<()> {
        self.mutate_state(|raw| {
            remove_id_from_nested_arrays(raw, id);
            object_field(raw, "global_blocked").remove(id);
            if let Some(groups) = raw.get_mut("group_blocked").and_then(Value::as_object_mut) {
                for group in groups.values_mut().filter_map(Value::as_object_mut) {
                    group.remove(id);
                }
            }
            Ok(())
        })
    }

    fn migrate_legacy(&self) -> io::Result<()> {
        let legacy_path = self.home.root().join("capabilities.json");
        let marker = self
            .home
            .root()
            .join("state/capabilities/.rust-capabilities-migrated-v1");
        if marker.exists() || !legacy_path.exists() {
            return Ok(());
        }
        with_exclusive_lock(
            &self.home.root().join("state/capabilities/.migration.lock"),
            || {
                if marker.exists() {
                    return Ok(());
                }
                let legacy: CapabilityState = read_json(&legacy_path)?;
                let state_path = self.path();
                let mut state = if state_path.exists() {
                    read_json::<Value>(&state_path)?
                } else {
                    json!({"v":1,"created_at":utc_now()})
                };
                let blocked = object_field(&mut state, "global_blocked");
                for id in legacy.blocked {
                    blocked.entry(id).or_insert_with(|| {
                        json!({"reason":"","by":"migration","blocked_at":utc_now(),"expires_at":""})
                    });
                }
                let group_ids = crate::GroupStore::new(self.home.clone())?
                    .list()?
                    .into_iter()
                    .map(|group| group.group_id)
                    .collect::<Vec<_>>();
                let enabled = object_field(&mut state, "group_enabled");
                for group_id in group_ids {
                    let items = enabled.entry(group_id).or_insert_with(|| json!([]));
                    for id in &legacy.enabled {
                        set_array_member(items, id, true);
                    }
                    for id in &legacy.disabled {
                        set_array_member(items, id, false);
                    }
                }
                state["updated_at"] = json!(utc_now());
                write_json(&state_path, &state)?;

                if !legacy.custom.is_empty() {
                    let catalog_path = self.catalog_path();
                    let mut catalog = if catalog_path.exists() {
                        read_json::<Value>(&catalog_path)?
                    } else {
                        json!({"v":1,"created_at":utc_now(),"sources":{},"records":{}})
                    };
                    let records = object_field(&mut catalog, "records");
                    for (_, capability) in legacy.custom {
                        records.entry(capability.id.clone()).or_insert_with(|| {
                            json!({
                                "capability_id":capability.id,
                                "kind":capability.kind,
                                "name":capability.name,
                                "description_short":capability.description,
                                "tool_names":capability.tool_names,
                                "tags":capability.tags,
                                "capsule_text":capability.capsule_text,
                                "source_id":if capability.source.is_empty(){"manual_import"}else{&capability.source},
                                "source_uri":capability.source_uri,
                                "qualification_status":"qualified",
                                "enable_supported":true
                            })
                        });
                    }
                    catalog["updated_at"] = json!(utc_now());
                    write_json(&catalog_path, &catalog)?;
                }
                if let Some(parent) = marker.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(marker, b"migrated from capabilities.json\n")
            },
        )
    }

    fn path(&self) -> std::path::PathBuf {
        self.home.root().join("state/capabilities/state.json")
    }

    fn catalog_path(&self) -> std::path::PathBuf {
        self.home.root().join("state/capabilities/catalog.json")
    }
}

fn object_field<'a>(value: &'a mut Value, field: &str) -> &'a mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("object initialized");
    let field = root.entry(field).or_insert_with(|| json!({}));
    if !field.is_object() {
        *field = json!({});
    }
    field.as_object_mut().expect("field object initialized")
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized")
}

fn set_array_member(value: &mut Value, id: &str, present: bool) {
    if !value.is_array() {
        *value = json!([]);
    }
    let items = value.as_array_mut().expect("array initialized");
    items.retain(|item| item.as_str() != Some(id));
    if present {
        items.push(json!(id));
        items.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
}

fn remove_empty_entry(map: &mut Map<String, Value>, key: &str) {
    if map.get(key).is_some_and(|value| {
        value.as_array().is_some_and(Vec::is_empty) || value.as_object().is_some_and(Map::is_empty)
    }) {
        map.remove(key);
    }
}

fn remove_id_from_nested_arrays(value: &mut Value, id: &str) {
    match value {
        Value::Array(items) => items.retain(|item| item.as_str() != Some(id)),
        Value::Object(items) => {
            for value in items.values_mut() {
                remove_id_from_nested_arrays(value, id);
            }
        }
        _ => {}
    }
}

fn validate_id(id: &str) -> io::Result<()> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::other("invalid capability id"))
    }
}

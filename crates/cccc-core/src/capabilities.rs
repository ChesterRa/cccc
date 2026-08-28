use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    #[serde(default = "default_qualification_status")]
    pub qualification_status: String,
    #[serde(default = "default_enable_supported")]
    pub enable_supported: bool,
}

fn default_qualification_status() -> String {
    "qualified".into()
}

const fn default_enable_supported() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct CapabilityStore {
    home: HomeLayout,
}

enum BindingMutation {
    SetEnabled(bool),
    EnableAndUnhide,
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
            let raw = read_document_object(&path)?;
            let mut state = CapabilityState::default();
            state.blocked.extend(
                raw.get("global_blocked")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter(|(_, entry)| block_entry_is_active(entry))
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
        let record = json!({
            "capability_id":capability.id,
            "kind":capability.kind,
            "name":capability.name,
            "description_short":capability.description,
            "tool_names":capability.tool_names,
            "tags":capability.tags,
            "capsule_text":capability.capsule_text,
            "source_id":if capability.source.is_empty(){"manual_import"}else{&capability.source},
            "source_uri":capability.source_uri,
            "qualification_status":capability.qualification_status,
            "enable_supported":capability.enable_supported
        });
        self.import_record(record)?;
        self.load()
    }

    pub fn import_record(&self, mut record: Value) -> io::Result<Capability> {
        let capability = self.validate_record(&record)?;
        let record = record
            .as_object_mut()
            .ok_or_else(|| io::Error::other("capability record must be an object"))?;
        record.insert("capability_id".into(), json!(&capability.id));
        record
            .entry("source_id")
            .or_insert_with(|| json!("manual_import"));
        record
            .entry("qualification_status")
            .or_insert_with(|| json!("qualified"));
        record
            .entry("enable_supported")
            .or_insert_with(|| json!(true));
        let path = self.catalog_path();
        with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut raw = if path.exists() {
                read_document_object(&path)?
            } else {
                json!({"v":1,"created_at":utc_now(),"sources":{},"records":{}})
            };
            raw["updated_at"] = json!(utc_now());
            object_field(&mut raw, "records")
                .insert(capability.id.clone(), Value::Object(record.clone()));
            write_json(&path, &raw)
        })?;
        Ok(capability)
    }

    pub fn validate_record(&self, record: &Value) -> io::Result<Capability> {
        let capability = capability_from_record(record)?;
        validate_id(&capability.id)?;
        Ok(capability)
    }

    pub fn restore_record(&self, id: &str, record: Option<Value>) -> io::Result<()> {
        match record {
            Some(mut record) => {
                record
                    .as_object_mut()
                    .ok_or_else(|| io::Error::other("capability record must be an object"))?
                    .insert("capability_id".into(), json!(id));
                self.import_record(record).map(|_| ())
            }
            None => self.remove_record(id).map(|_| ()),
        }
    }

    pub fn uninstall(&self, id: &str) -> io::Result<bool> {
        let removed = self.remove_record(id)?;
        self.remove_all_bindings(id)?;
        Ok(removed)
    }

    pub fn remove_record(&self, id: &str) -> io::Result<bool> {
        let path = self.catalog_path();
        with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut raw = if path.exists() {
                read_document_object(&path)?
            } else {
                json!({})
            };
            let removed = object_field(&mut raw, "records").remove(id).is_some();
            raw["updated_at"] = json!(utc_now());
            write_json(&path, &raw)?;
            Ok(removed)
        })
    }

    pub fn remove_bindings_for_group(&self, id: &str, group_id: &str) -> io::Result<usize> {
        self.mutate_state(|raw| Ok(remove_enabled_bindings(raw, id, Some(group_id))))
    }

    pub fn uninstall_for_group(&self, id: &str, group_id: &str) -> io::Result<(usize, bool, bool)> {
        if group_id.is_empty() {
            return Err(io::Error::other("group_id is required"));
        }
        self.mutate_state(|raw| {
            let removed_bindings = remove_enabled_bindings(raw, id, Some(group_id));
            let groups = object_field(raw, "group_removed");
            let items = groups.entry(group_id).or_insert_with(|| json!([]));
            let marker_changed = !array_contains_id(items, id);
            set_array_member(items, id, true);
            let has_remaining_bindings = ["group_enabled", "actor_enabled", "session_enabled"]
                .iter()
                .filter_map(|key| raw.get(*key))
                .any(|value| contains_id(value, id));
            Ok((removed_bindings, marker_changed, has_remaining_bindings))
        })
    }

    pub fn remove_all_bindings(&self, id: &str) -> io::Result<usize> {
        self.mutate_state(|raw| Ok(remove_id(raw, id)))
    }

    pub fn has_bindings(&self, id: &str) -> io::Result<bool> {
        let path = self.path();
        if !path.exists() {
            return Ok(false);
        }
        let raw = read_document_object(&path)?;
        Ok(["group_enabled", "actor_enabled", "session_enabled"]
            .iter()
            .filter_map(|key| raw.get(*key))
            .any(|value| contains_id(value, id)))
    }

    pub fn removed_for_group(&self, group_id: &str) -> io::Result<BTreeSet<String>> {
        let path = self.path();
        if group_id.is_empty() || !path.exists() {
            return Ok(BTreeSet::new());
        }
        let raw = read_document_object(&path)?;
        Ok(raw
            .pointer(&format!("/group_removed/{}", escape_pointer(group_id)))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect())
    }

    pub fn set_removed_for_group(
        &self,
        id: &str,
        group_id: &str,
        removed: bool,
    ) -> io::Result<bool> {
        if group_id.is_empty() {
            return Err(io::Error::other("group_id is required"));
        }
        self.mutate_state(|raw| {
            let groups = object_field(raw, "group_removed");
            let items = groups.entry(group_id).or_insert_with(|| json!([]));
            let was_present = array_contains_id(items, id);
            set_array_member(items, id, removed);
            remove_empty_entry(groups, group_id);
            Ok(was_present != removed)
        })
    }

    pub fn is_enabled_for(
        &self,
        id: &str,
        group_id: &str,
        actor_id: &str,
        scope: &str,
    ) -> io::Result<bool> {
        let path = self.path();
        if !path.exists() {
            return Ok(false);
        }
        let raw = read_document_object(&path)?;
        let value = match scope {
            "group" => raw.pointer(&format!("/group_enabled/{}", escape_pointer(group_id))),
            "actor" => raw.pointer(&format!(
                "/actor_enabled/{}/{}",
                escape_pointer(group_id),
                escape_pointer(actor_id)
            )),
            "session" => raw.pointer(&format!(
                "/session_enabled/{}/{}",
                escape_pointer(group_id),
                escape_pointer(actor_id)
            )),
            _ => return Err(io::Error::other("scope must be group, actor, or session")),
        };
        Ok(value.and_then(Value::as_array).is_some_and(|items| {
            items.iter().any(|item| {
                item.as_str() == Some(id)
                    || item.get("capability_id").and_then(Value::as_str) == Some(id)
            })
        }))
    }

    pub fn is_hidden_for(&self, id: &str, group_id: &str, actor_id: &str) -> io::Result<bool> {
        let path = self.path();
        if !path.exists() {
            return Ok(false);
        }
        let raw = read_document_object(&path)?;
        Ok(raw
            .pointer(&format!(
                "/actor_hidden/{}/{}",
                escape_pointer(group_id),
                escape_pointer(actor_id)
            ))
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(id))))
    }

    pub fn blocked_for_group(
        &self,
        id: &str,
        group_id: &str,
    ) -> io::Result<Option<(String, Value)>> {
        let path = self.path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = read_document_object(&path)?;
        if let Some(entry) = active_block_entry(raw.get("global_blocked"), id) {
            return Ok(Some(("global".into(), entry)));
        }
        Ok(active_block_entry(
            raw.pointer(&format!("/group_blocked/{}", escape_pointer(group_id))),
            id,
        )
        .map(|entry| ("group".into(), entry)))
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
        self.mutate_binding_for(
            id,
            BindingMutation::SetEnabled(enabled),
            group_id,
            actor_id,
            scope,
            ttl_seconds,
        )
    }

    pub fn seed_default_group_capabilities(&self, group_id: &str) -> io::Result<bool> {
        use crate::capability_builtin::{
            DEFAULT_GROUP_CAPABILITY_SEED_VERSION, LEGACY_SELF_EVOLUTION_CAPABILITY_ID,
            SELF_EVOLUTION_CAPABILITY_ID,
        };

        if group_id.is_empty() {
            return Ok(false);
        }
        self.mutate_state(|raw| {
            let current_version = raw
                .pointer(&format!(
                    "/default_group_capability_seed_versions/{}",
                    escape_pointer(group_id)
                ))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if current_version >= DEFAULT_GROUP_CAPABILITY_SEED_VERSION {
                return Ok(false);
            }

            let legacy_explicitly_removed = raw
                .pointer(&format!("/group_removed/{}", escape_pointer(group_id)))
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.as_str() == Some(LEGACY_SELF_EVOLUTION_CAPABILITY_ID))
                });
            migrate_capability_controls(
                raw,
                group_id,
                LEGACY_SELF_EVOLUTION_CAPABILITY_ID,
                SELF_EVOLUTION_CAPABILITY_ID,
            );
            let legacy_removed =
                remove_enabled_bindings(raw, LEGACY_SELF_EVOLUTION_CAPABILITY_ID, Some(group_id));
            if legacy_removed > 0 {
                let groups = object_field(raw, "group_removed");
                let items = groups.entry(group_id).or_insert_with(|| json!([]));
                set_array_member(items, LEGACY_SELF_EVOLUTION_CAPABILITY_ID, true);
            }

            if legacy_explicitly_removed {
                let groups = object_field(raw, "group_removed");
                let items = groups.entry(group_id).or_insert_with(|| json!([]));
                set_array_member(items, SELF_EVOLUTION_CAPABILITY_ID, true);
            }

            let self_evolution_removed = raw
                .pointer(&format!("/group_removed/{}", escape_pointer(group_id)))
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.as_str() == Some(SELF_EVOLUTION_CAPABILITY_ID))
                });
            if !self_evolution_removed {
                let groups = object_field(raw, "group_enabled");
                let items = groups.entry(group_id).or_insert_with(|| json!([]));
                set_array_member(items, SELF_EVOLUTION_CAPABILITY_ID, true);
            }

            object_field(raw, "default_group_capability_seed_versions").insert(
                group_id.into(),
                json!(DEFAULT_GROUP_CAPABILITY_SEED_VERSION),
            );
            Ok(true)
        })
    }

    pub fn enable_and_unhide_for(
        &self,
        id: &str,
        group_id: &str,
        actor_id: &str,
        scope: &str,
        ttl_seconds: i64,
    ) -> io::Result<CapabilityState> {
        self.mutate_binding_for(
            id,
            BindingMutation::EnableAndUnhide,
            group_id,
            actor_id,
            scope,
            ttl_seconds,
        )
    }

    fn mutate_binding_for(
        &self,
        id: &str,
        mutation: BindingMutation,
        group_id: &str,
        actor_id: &str,
        scope: &str,
        ttl_seconds: i64,
    ) -> io::Result<CapabilityState> {
        self.require(id)?;
        if group_id.is_empty() {
            return Err(io::Error::other("group_id is required"));
        }
        let (enabled, unhide) = match mutation {
            BindingMutation::SetEnabled(enabled) => (enabled, false),
            BindingMutation::EnableAndUnhide => (true, true),
        };
        self.mutate_state(|raw| {
            if enabled {
                let groups = object_field(raw, "group_removed");
                if let Some(items) = groups.get_mut(group_id) {
                    set_array_member(items, id, false);
                    remove_empty_entry(groups, group_id);
                }
            }
            match scope {
                "group" => {
                    if id == crate::capability_builtin::SELF_EVOLUTION_CAPABILITY_ID {
                        object_field(raw, "default_group_capability_seed_versions").insert(
                            group_id.into(),
                            json!(crate::capability_builtin::DEFAULT_GROUP_CAPABILITY_SEED_VERSION),
                        );
                    }
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
            if unhide && !actor_id.is_empty() {
                let groups = object_field(raw, "actor_hidden");
                if let Some(group) = groups.get_mut(group_id).and_then(Value::as_object_mut) {
                    if let Some(items) = group.get_mut(actor_id) {
                        set_array_member(items, id, false);
                    }
                    remove_empty_entry(group, actor_id);
                }
                remove_empty_entry(groups, group_id);
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
        self.set_blocked_and_revoke_for(id, blocked, group_id, reason, by, ttl_seconds)
            .map(|(state, _, _)| state)
    }

    pub fn set_blocked_and_revoke_for(
        &self,
        id: &str,
        blocked: bool,
        group_id: &str,
        reason: &str,
        by: &str,
        ttl_seconds: i64,
    ) -> io::Result<(CapabilityState, usize, Option<Value>)> {
        self.require(id)?;
        let block_entry = blocked.then(|| {
            let expires_at = if ttl_seconds > 0 {
                (chrono::Utc::now()
                    + chrono::Duration::seconds(ttl_seconds.clamp(1, 30 * 24 * 3600)))
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
            } else {
                String::new()
            };
            json!({
                "reason":reason.chars().take(280).collect::<String>(),
                "by":by,
                "blocked_at":utc_now(),
                "expires_at":expires_at,
            })
        });
        let removed_bindings = self.mutate_state(|raw| {
            {
                let target = if group_id.is_empty() {
                    object_field(raw, "global_blocked")
                } else {
                    let groups = object_field(raw, "group_blocked");
                    ensure_object(groups.entry(group_id).or_insert_with(|| json!({})))
                };
                if let Some(entry) = block_entry.as_ref() {
                    target.insert(id.into(), entry.clone());
                } else {
                    target.remove(id);
                }
            }
            Ok(if blocked {
                remove_enabled_bindings(raw, id, (!group_id.is_empty()).then_some(group_id))
            } else {
                0
            })
        })?;
        Ok((self.load()?, removed_bindings, block_entry))
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
                read_document_object(&path)?
            } else {
                json!({"v":1,"created_at":utc_now()})
            };
            let original = raw.clone();
            let result = change(&mut raw)?;
            raw["v"] = json!(1);
            if raw != original {
                raw["updated_at"] = json!(utc_now());
                write_json(&path, &raw)?;
            }
            Ok(result)
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
                    read_document_object(&state_path)?
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
                        read_document_object(&catalog_path)?
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

fn read_document_object(path: &std::path::Path) -> io::Result<Value> {
    let value = read_json::<Value>(path)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(io::Error::other(format!(
            "shared capability document must be an object: {}",
            path.display()
        )))
    }
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

fn array_contains_id(value: &Value, id: &str) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(id)))
}

fn migrate_capability_controls(raw: &mut Value, group_id: &str, legacy_id: &str, new_id: &str) {
    let global_block = raw
        .get("global_blocked")
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(legacy_id))
        .cloned();
    if let Some(entry) = global_block {
        object_field(raw, "global_blocked")
            .entry(new_id)
            .or_insert(entry);
    }

    let group_block = raw
        .get("group_blocked")
        .and_then(Value::as_object)
        .and_then(|groups| groups.get(group_id))
        .and_then(Value::as_object)
        .and_then(|entries| entries.get(legacy_id))
        .cloned();
    if let Some(entry) = group_block {
        let groups = object_field(raw, "group_blocked");
        ensure_object(groups.entry(group_id).or_insert_with(|| json!({})))
            .entry(new_id)
            .or_insert(entry);
    }

    let Some(hidden_by_actor) = raw
        .get_mut("actor_hidden")
        .and_then(Value::as_object_mut)
        .and_then(|groups| groups.get_mut(group_id))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for hidden in hidden_by_actor.values_mut() {
        if array_contains_id(hidden, legacy_id) {
            set_array_member(hidden, new_id, true);
        }
    }
}

fn active_block_entry(value: Option<&Value>, id: &str) -> Option<Value> {
    match value {
        Some(Value::Object(entries)) => entries
            .get(id)
            .filter(|entry| block_entry_is_active(entry))
            .cloned(),
        Some(Value::Array(entries)) if entries.iter().any(|entry| entry.as_str() == Some(id)) => {
            Some(json!({"reason":"","by":"","blocked_at":"","expires_at":""}))
        }
        _ => None,
    }
}

pub(crate) fn block_entry_is_active(entry: &Value) -> bool {
    entry
        .get("expires_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|expires_at| expires_at > chrono::Utc::now())
}

fn remove_empty_entry(map: &mut Map<String, Value>, key: &str) {
    if map.get(key).is_some_and(|value| {
        value.as_array().is_some_and(Vec::is_empty) || value.as_object().is_some_and(Map::is_empty)
    }) {
        map.remove(key);
    }
}

fn remove_enabled_bindings(raw: &mut Value, id: &str, group_id: Option<&str>) -> usize {
    let mut removed = 0;
    for key in ["group_enabled", "actor_enabled", "session_enabled"] {
        let Some(groups) = raw.get_mut(key).and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(group_id) = group_id {
            if let Some(group) = groups.get_mut(group_id) {
                removed += remove_id(group, id);
                remove_empty_entry(groups, group_id);
            }
        } else {
            removed += groups
                .values_mut()
                .map(|value| remove_id(value, id))
                .sum::<usize>();
            groups.retain(|_, value| {
                !value.as_array().is_some_and(Vec::is_empty)
                    && !value.as_object().is_some_and(Map::is_empty)
            });
        }
    }
    removed
}

fn remove_id(value: &mut Value, id: &str) -> usize {
    match value {
        Value::Array(items) => {
            let before = items.len();
            items.retain(|item| {
                item.as_str() != Some(id)
                    && item.get("capability_id").and_then(Value::as_str) != Some(id)
            });
            let direct = before - items.len();
            direct
                + items
                    .iter_mut()
                    .map(|item| remove_id(item, id))
                    .sum::<usize>()
        }
        Value::Object(items) => {
            let direct = usize::from(items.remove(id).is_some());
            direct
                + items
                    .values_mut()
                    .map(|item| remove_id(item, id))
                    .sum::<usize>()
        }
        _ => 0,
    }
}

fn contains_id(value: &Value, id: &str) -> bool {
    match value {
        Value::String(value) => value == id,
        Value::Array(items) => items.iter().any(|item| contains_id(item, id)),
        Value::Object(items) => {
            items.contains_key(id)
                || items
                    .get("capability_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == id)
                || items.values().any(|item| contains_id(item, id))
        }
        _ => false,
    }
}

fn capability_from_record(record: &Value) -> io::Result<Capability> {
    let id = record
        .get("capability_id")
        .or_else(|| record.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("capability_id is required"))?;
    let strings = |key: &str| {
        record
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    Ok(Capability {
        id: id.to_owned(),
        kind: record
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        name: record
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_owned(),
        description: record
            .get("description_short")
            .or_else(|| record.get("description"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        tool_names: strings("tool_names"),
        tags: strings("tags"),
        capsule_text: record
            .get("capsule_text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        source: record
            .get("source_id")
            .and_then(Value::as_str)
            .unwrap_or("manual_import")
            .to_owned(),
        source_uri: record
            .get("source_uri")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        qualification_status: record
            .get("qualification_status")
            .and_then(Value::as_str)
            .unwrap_or("qualified")
            .to_owned(),
        enable_supported: record
            .get("enable_supported")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
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

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, CapabilityStore) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        (temp, CapabilityStore::new(home))
    }

    #[test]
    fn import_record_preserves_install_metadata() {
        let (_temp, store) = store();
        store
            .import_record(json!({
                "capability_id":"mcp:test","kind":"mcp","name":"Test",
                "description_short":"test capability","source_id":"github_import",
                "install_mode":"npm","install_spec":{"package":"test-server"},
                "qualification_status":"qualified","enable_supported":true
            }))
            .expect("import");
        let record = store
            .catalog_record("mcp:test")
            .expect("catalog")
            .expect("record");
        assert_eq!(record["install_spec"]["package"], "test-server");
        assert_eq!(record["source_id"], "github_import");
    }

    #[test]
    fn enable_and_unhide_updates_one_state_document() {
        let (_temp, store) = store();
        store
            .import_record(json!({
                "capability_id":"skill:test","kind":"skill","name":"Test",
                "capsule_text":"Test skill"
            }))
            .expect("import");
        store
            .set_hidden_for("skill:test", true, "g_test", "user")
            .expect("hide");

        store
            .enable_and_unhide_for("skill:test", "g_test", "user", "group", 3600)
            .expect("enable and unhide");

        assert!(
            store
                .is_enabled_for("skill:test", "g_test", "user", "group")
                .expect("enabled")
        );
        assert!(
            !store
                .is_hidden_for("skill:test", "g_test", "user")
                .expect("visible")
        );
    }

    #[test]
    fn group_uninstall_keeps_other_group_bindings() {
        let (_temp, store) = store();
        store
            .import_record(json!({"capability_id":"skill:test","name":"Test"}))
            .expect("import");
        store
            .set_enabled_for("skill:test", true, "g_one", "", "group", 3600)
            .expect("first binding");
        store
            .set_enabled_for("skill:test", true, "g_two", "actor", "actor", 3600)
            .expect("second binding");
        store
            .set_hidden_for("skill:test", true, "g_one", "actor")
            .expect("hidden preference");
        assert_eq!(
            store
                .set_blocked_and_revoke_for("skill:test", true, "g_one", "reason", "user", 0)
                .expect("group block")
                .1,
            1
        );
        assert!(
            store
                .is_hidden_for("skill:test", "g_one", "actor")
                .expect("hidden preserved")
        );
        assert!(
            store
                .set_removed_for_group("skill:test", "g_one", true)
                .expect("removed marker")
        );
        assert!(store.has_bindings("skill:test").expect("remaining"));
        assert_eq!(
            store
                .remove_bindings_for_group("skill:test", "g_two")
                .expect("remove other group"),
            1
        );
        assert!(!store.has_bindings("skill:test").expect("empty"));
        assert_eq!(
            store.removed_for_group("g_one").expect("removed ids"),
            BTreeSet::from(["skill:test".to_owned()])
        );
    }

    #[test]
    fn repeated_default_seed_does_not_rewrite_state() {
        let (_temp, store) = store();
        assert!(
            store
                .seed_default_group_capabilities("g_test")
                .expect("first seed")
        );
        let before = std::fs::read(store.path()).expect("state before repeated seed");

        assert!(
            !store
                .seed_default_group_capabilities("g_test")
                .expect("repeated seed")
        );

        assert_eq!(
            std::fs::read(store.path()).expect("state after repeated seed"),
            before
        );
    }
}

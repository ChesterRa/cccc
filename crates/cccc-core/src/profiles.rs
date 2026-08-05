use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use uuid::Uuid;

use crate::fs::{read_json, with_exclusive_lock, write_json, write_secret_json};
use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone)]
pub struct ProfileStore {
    home: HomeLayout,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileDoc {
    #[serde(default = "profile_version")]
    v: u8,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    profiles: BTreeMap<String, Value>,
}

impl Default for ProfileDoc {
    fn default() -> Self {
        let now = utc_now();
        Self {
            v: profile_version(),
            created_at: now.clone(),
            updated_at: now,
            profiles: BTreeMap::new(),
        }
    }
}

const fn profile_version() -> u8 {
    1
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretDoc {
    #[serde(default)]
    profiles: BTreeMap<String, BTreeMap<String, String>>,
}

impl ProfileStore {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        let store = Self { home };
        store.migrate_legacy_env()?;
        Ok(store)
    }

    pub fn list(&self) -> io::Result<Vec<Value>> {
        let mut profiles = self.load()?.profiles.into_values().collect::<Vec<_>>();
        for profile in &mut profiles {
            profile["usage_count"] = json!(
                self.usage_ref(
                    profile["id"].as_str().unwrap_or(""),
                    profile["scope"].as_str().unwrap_or("global"),
                    profile["owner_id"].as_str().unwrap_or(""),
                )?
                .len()
            );
        }
        profiles.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(profiles)
    }

    pub fn remove_capability_default(&self, capability_id: &str) -> io::Result<usize> {
        let mut doc = self.load()?;
        let mut removed = 0;
        for profile in doc.profiles.values_mut() {
            let Some(items) = profile
                .pointer_mut("/capability_defaults/autoload_capabilities")
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            let before = items.len();
            items.retain(|item| item.as_str() != Some(capability_id));
            removed += before - items.len();
            if before != items.len() {
                profile["updated_at"] = json!(utc_now());
                profile["revision"] = json!(profile["revision"].as_u64().unwrap_or(0) + 1);
            }
        }
        if removed > 0 {
            self.save(&doc)?;
        }
        Ok(removed)
    }

    pub fn get(&self, profile_id: &str) -> io::Result<Option<Value>> {
        validate_id(profile_id)?;
        let doc = self.load()?;
        Ok(find_profile(&doc.profiles, profile_id).map(|(_, value)| value.clone()))
    }

    pub fn get_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
    ) -> io::Result<Option<Value>> {
        validate_profile_ref(profile_id, scope, owner_id)?;
        let key =
            serde_json::to_string(&(scope, owner_id, profile_id)).map_err(io::Error::other)?;
        let doc = self.load()?;
        Ok(doc
            .profiles
            .get(&key)
            .or_else(|| {
                (scope == "global" && owner_id.is_empty())
                    .then(|| doc.profiles.get(profile_id))
                    .flatten()
            })
            .cloned())
    }

    pub fn upsert(
        &self,
        mut profile: Map<String, Value>,
        expected: Option<u64>,
    ) -> io::Result<Value> {
        let mut doc = self.load()?;
        let id = profile
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("ap_{}", &Uuid::new_v4().simple().to_string()[..12]));
        let scope = profile
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("global")
            .to_owned();
        let owner_id = profile
            .get("owner_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        validate_profile_ref(&id, &scope, &owner_id)?;
        let storage_key =
            serde_json::to_string(&(&scope, &owner_id, &id)).map_err(io::Error::other)?;
        let current_key = doc
            .profiles
            .contains_key(&storage_key)
            .then_some(storage_key)
            .or_else(|| {
                (scope == "global" && owner_id.is_empty() && doc.profiles.contains_key(&id))
                    .then_some(id.clone())
            });
        let current = current_key.as_ref().and_then(|key| doc.profiles.get(key));
        let revision = current
            .and_then(|value| value["revision"].as_u64())
            .unwrap_or(0);
        if expected.is_some_and(|expected| expected != revision) {
            return Err(io::Error::other(format!(
                "revision conflict: expected {}, current {revision}",
                expected.unwrap_or_default()
            )));
        }
        let env = profile
            .remove("env")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| (key, json!(python_string(&value))))
            .collect::<Map<_, _>>();
        let now = utc_now();
        profile.insert("v".into(), json!(1));
        profile.insert("id".into(), json!(id));
        profile.entry("name").or_insert_with(|| json!(""));
        profile.insert("scope".into(), json!(scope));
        profile.insert("owner_id".into(), json!(owner_id));
        profile.entry("runtime").or_insert_with(|| json!("codex"));
        profile.entry("runner").or_insert_with(|| json!("pty"));
        profile.entry("command").or_insert_with(|| json!([]));
        profile.entry("submit").or_insert_with(|| json!("enter"));
        profile.insert("env".into(), Value::Object(env));
        profile.insert(
            "created_at".into(),
            current
                .and_then(|value| value["created_at"].as_str())
                .map_or_else(|| json!(now), |value| json!(value)),
        );
        profile.insert("updated_at".into(), json!(utc_now()));
        profile.insert("revision".into(), json!(revision + 1));
        profile.retain(|key, _| {
            matches!(
                key.as_str(),
                "v" | "id"
                    | "name"
                    | "scope"
                    | "owner_id"
                    | "runtime"
                    | "runner"
                    | "command"
                    | "submit"
                    | "env"
                    | "created_at"
                    | "updated_at"
                    | "revision"
                    | "capability_defaults"
            )
        });
        let result = Value::Object(profile);
        if let Some(current_key) = current_key {
            doc.profiles.remove(&current_key);
        }
        doc.profiles
            .insert(profile_storage_key(&result), result.clone());
        self.save(&doc)?;
        Ok(result)
    }

    pub fn delete(&self, profile_id: &str, force_detach: bool) -> io::Result<(bool, Vec<Value>)> {
        self.delete_ref(profile_id, "global", "", force_detach)
    }

    pub fn delete_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
        force_detach: bool,
    ) -> io::Result<(bool, Vec<Value>)> {
        validate_profile_ref(profile_id, scope, owner_id)?;
        let usage = self.usage_ref(profile_id, scope, owner_id)?;
        if !usage.is_empty() && !force_detach {
            return Err(io::Error::other(
                "profile is in use; force_detach is required",
            ));
        }
        if force_detach {
            let groups = GroupStore::new(self.home.clone())?;
            for entry in &usage {
                let group_id = entry["group_id"].as_str().unwrap_or("");
                let actor_id = entry["actor_id"].as_str().unwrap_or("");
                groups.mutate(group_id, |group| {
                    if let Some(actor) = group.actors.iter_mut().find(|actor| actor.id == actor_id)
                    {
                        actor.profile_id.clear();
                        actor.profile_scope = "global".into();
                        actor.profile_owner.clear();
                        actor.profile_revision_applied = 0;
                    }
                    Ok(())
                })?;
            }
        }
        let mut doc = self.load()?;
        let key =
            serde_json::to_string(&(scope, owner_id, profile_id)).map_err(io::Error::other)?;
        let deleted_profile = doc.profiles.get(&key).cloned().or_else(|| {
            (scope == "global" && owner_id.is_empty())
                .then(|| doc.profiles.get(profile_id).cloned())
                .flatten()
        });
        let mut deleted = doc.profiles.remove(&key).is_some();
        if scope == "global" && owner_id.is_empty() {
            deleted |= doc.profiles.remove(profile_id).is_some();
        }
        self.save(&doc)?;
        if let Some(profile) = deleted_profile {
            self.write_secret_file(&profile, &BTreeMap::new())?;
        }
        Ok((deleted, usage))
    }

    pub fn usage(&self, profile_id: &str) -> io::Result<Vec<Value>> {
        self.usage_ref(profile_id, "global", "")
    }

    pub fn usage_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
    ) -> io::Result<Vec<Value>> {
        validate_profile_ref(profile_id, scope, owner_id)?;
        let groups = GroupStore::new(self.home.clone())?;
        let mut usage = Vec::new();
        for meta in groups.list()? {
            if let Ok(group) = groups.load(&meta.group_id) {
                for actor in group.actors.iter().filter(|actor| {
                    actor.profile_id == profile_id
                        && actor.profile_scope == scope
                        && actor.profile_owner == owner_id
                }) {
                    usage.push(json!({"group_id":group.group_id,"group_title":group.title,"actor_id":actor.id,"actor_title":actor.title}));
                }
            }
        }
        Ok(usage)
    }

    pub fn secret_keys(&self, profile_id: &str) -> io::Result<Vec<String>> {
        self.secret_keys_ref(profile_id, "global", "")
    }
    pub fn secret_keys_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
    ) -> io::Result<Vec<String>> {
        Ok(self
            .secret_values_ref(profile_id, scope, owner_id)?
            .into_keys()
            .collect())
    }
    pub fn secret_values(&self, profile_id: &str) -> io::Result<BTreeMap<String, String>> {
        self.secret_values_ref(profile_id, "global", "")
    }
    pub fn secret_values_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
    ) -> io::Result<BTreeMap<String, String>> {
        let profile = self
            .get_ref(profile_id, scope, owner_id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "profile not found"))?;
        let path = self.secret_path(&profile);
        if path.exists() {
            read_json(&path)
        } else {
            Ok(BTreeMap::new())
        }
    }
    pub fn update_secrets(
        &self,
        profile_id: &str,
        set: &Map<String, Value>,
        unset: &[Value],
        clear: bool,
    ) -> io::Result<Vec<String>> {
        self.update_secrets_ref(profile_id, "global", "", set, unset, clear)
    }
    pub fn update_secrets_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
        set: &Map<String, Value>,
        unset: &[Value],
        clear: bool,
    ) -> io::Result<Vec<String>> {
        let profile = self
            .get_ref(profile_id, scope, owner_id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "profile not found"))?;
        let mut values = self.secret_values_ref(profile_id, scope, owner_id)?;
        if clear {
            values.clear();
        }
        for (key, value) in set {
            values.insert(key.clone(), python_string(value));
        }
        for key in unset {
            values.remove(&python_string(key));
        }
        let keys = values.keys().cloned().collect();
        self.write_secret_file(&profile, &values)?;
        Ok(keys)
    }
    pub fn replace_secrets(
        &self,
        profile_id: &str,
        values: BTreeMap<String, String>,
    ) -> io::Result<Vec<String>> {
        self.replace_secrets_ref(profile_id, "global", "", values)
    }
    pub fn replace_secrets_ref(
        &self,
        profile_id: &str,
        scope: &str,
        owner_id: &str,
        values: BTreeMap<String, String>,
    ) -> io::Result<Vec<String>> {
        let profile = self
            .get_ref(profile_id, scope, owner_id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "profile not found"))?;
        let keys = values.keys().cloned().collect();
        self.write_secret_file(&profile, &values)?;
        Ok(keys)
    }

    fn load(&self) -> io::Result<ProfileDoc> {
        let path = self.home.root().join("state/actor_profiles/profiles.json");
        if path.exists() {
            read_json(&path)
        } else {
            Ok(ProfileDoc::default())
        }
    }
    fn save(&self, value: &ProfileDoc) -> io::Result<()> {
        let mut value = ProfileDoc {
            v: value.v,
            created_at: value.created_at.clone(),
            updated_at: utc_now(),
            profiles: value.profiles.clone(),
        };
        if value.created_at.is_empty() {
            value.created_at = value.updated_at.clone();
        }
        write_json(
            &self.home.root().join("state/actor_profiles/profiles.json"),
            &value,
        )
    }

    fn secret_path(&self, profile: &Value) -> std::path::PathBuf {
        let id = profile["id"].as_str().unwrap_or_default();
        let scope = profile["scope"].as_str().unwrap_or("global");
        let owner = profile["owner_id"].as_str().unwrap_or_default();
        let filename = if scope == "global" && owner.is_empty() {
            hashed_filename(id, 32)
        } else {
            let storage_key = serde_json::to_string(&(scope, owner, id)).unwrap_or_default();
            let slug = format!("{owner}__{id}");
            hashed_filename_with_digest_source(&slug, &storage_key, 48)
        };
        self.home
            .root()
            .join("state/secrets/actor_profiles")
            .join(filename)
    }

    fn write_secret_file(
        &self,
        profile: &Value,
        values: &BTreeMap<String, String>,
    ) -> io::Result<()> {
        let path = self.secret_path(profile);
        if values.is_empty() {
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        } else {
            write_secret_json(&path, values)
        }
    }

    fn migrate_legacy_env(&self) -> io::Result<()> {
        let marker = self
            .home
            .root()
            .join("state/actor_profiles/.rust-profiles-migrated-v1");
        with_exclusive_lock(
            &self
                .home
                .root()
                .join("state/actor_profiles/.migration.lock"),
            || {
                let mut profiles = self.load()?;
                let mut changed = false;
                let mut imported_legacy_keys = BTreeSet::new();
                if !marker.exists() {
                    let legacy_profiles = self.home.root().join("profiles.json");
                    if legacy_profiles.exists() {
                        let legacy: ProfileDoc = read_json(&legacy_profiles)?;
                        for (key, profile) in legacy.profiles {
                            if find_profile(
                                &profiles.profiles,
                                profile["id"].as_str().unwrap_or(&key),
                            )
                            .is_none()
                            {
                                let storage_key = profile_storage_key(&profile);
                                imported_legacy_keys.insert(storage_key.clone());
                                profiles.profiles.insert(storage_key, profile);
                                changed = true;
                            }
                        }
                    }
                    let legacy_secrets = self.home.root().join("profile-secrets.json");
                    if legacy_secrets.exists() {
                        let secrets: SecretDoc = read_json(&legacy_secrets)?;
                        for (profile_id, values) in secrets.profiles {
                            if let Some(profile) = find_profile(&profiles.profiles, &profile_id)
                                .map(|(_, profile)| profile.clone())
                            {
                                let path = self.secret_path(&profile);
                                if !path.exists() && !values.is_empty() {
                                    write_secret_json(&path, &values)?;
                                }
                            }
                        }
                    }
                }
                let mut extracted = Vec::new();
                for (storage_key, profile) in &mut profiles.profiles {
                    if !imported_legacy_keys.contains(storage_key) {
                        continue;
                    }
                    let Some(env) = profile.get("env").and_then(Value::as_object).cloned() else {
                        continue;
                    };
                    if env.is_empty() {
                        continue;
                    }
                    let mut target = BTreeMap::new();
                    for (key, value) in env {
                        let value = value.as_str().ok_or_else(|| {
                            io::Error::other(format!(
                                "profile {storage_key} env values must be strings"
                            ))
                        })?;
                        target.insert(key, value.to_owned());
                    }
                    extracted.push((profile.clone(), target));
                    profile["env"] = json!({});
                    changed = true;
                }
                if changed {
                    self.save(&profiles)?;
                }
                for (profile, values) in extracted {
                    let mut current = if self.secret_path(&profile).exists() {
                        read_json::<BTreeMap<String, String>>(&self.secret_path(&profile))?
                    } else {
                        BTreeMap::new()
                    };
                    current.extend(values);
                    self.write_secret_file(&profile, &current)?;
                }
                if !marker.exists() {
                    if let Some(parent) = marker.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&marker, b"migrated from Rust profile storage\n")?;
                }
                Ok(())
            },
        )
    }
}

fn find_profile<'a>(
    profiles: &'a BTreeMap<String, Value>,
    profile_id: &str,
) -> Option<(&'a String, &'a Value)> {
    profiles
        .get_key_value(profile_id)
        .or_else(|| {
            profiles.iter().find(|(_, profile)| {
                profile["id"].as_str() == Some(profile_id)
                    && profile["scope"].as_str().unwrap_or("global") == "global"
            })
        })
        .or_else(|| {
            profiles
                .iter()
                .find(|(_, profile)| profile["id"].as_str() == Some(profile_id))
        })
}

fn profile_storage_key(profile: &Value) -> String {
    let id = profile["id"].as_str().unwrap_or_default();
    let scope = profile["scope"].as_str().unwrap_or("global");
    let owner = profile["owner_id"].as_str().unwrap_or_default();
    serde_json::to_string(&(scope, owner, id)).unwrap_or_else(|_| id.to_owned())
}

fn hashed_filename(value: &str, slug_limit: usize) -> String {
    hashed_filename_with_digest_source(value, value, slug_limit)
}

fn hashed_filename_with_digest_source(
    slug_source: &str,
    digest_source: &str,
    slug_limit: usize,
) -> String {
    let digest = format!("{:x}", Sha256::digest(digest_source.as_bytes()));
    let slug = slug_source
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .chars()
        .take(slug_limit)
        .collect::<String>();
    format!(
        "{}.{}.json",
        if slug.is_empty() { "profile" } else { &slug },
        &digest[..16]
    )
}

fn validate_id(value: &str) -> io::Result<()> {
    (!value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')))
    .then_some(())
    .ok_or_else(|| io::Error::other("invalid profile_id"))
}

fn validate_profile_ref(profile_id: &str, scope: &str, owner_id: &str) -> io::Result<()> {
    validate_id(profile_id)?;
    if !matches!(scope, "global" | "user") {
        return Err(io::Error::other("invalid profile scope"));
    }
    if scope == "global" && !owner_id.is_empty() {
        return Err(io::Error::other("global profile owner must be empty"));
    }
    if scope == "user" && owner_id.trim().is_empty() {
        return Err(io::Error::other("user scope profile requires owner_id"));
    }
    Ok(())
}

fn python_string(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(value) => if *value { "True" } else { "False" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => python_repr(value),
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(value) => if *value { "True" } else { "False" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| format!(
                    "'{}': {}",
                    key.replace('\\', "\\\\").replace('\'', "\\'"),
                    python_repr(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_store_migrates_legacy_env_before_profiles_are_returned() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        write_json(&home.root().join("profiles.json"),&json!({"profiles":{"legacy":{"id":"legacy","name":"Legacy","env":{"TOKEN":"secret"}}}})).expect("fixture");
        let store = ProfileStore::new(home.clone()).expect("store");
        assert_eq!(
            store.get("legacy").expect("get").expect("profile")["env"],
            json!({})
        );
        assert_eq!(
            store
                .secret_values("legacy")
                .expect("secrets")
                .get("TOKEN")
                .map(String::as_str),
            Some("secret")
        );
        assert!(
            !std::fs::read_to_string(home.root().join("state/actor_profiles/profiles.json"))
                .expect("profiles")
                .contains("secret")
        );
    }

    #[test]
    fn profile_secrets_use_python_string_coercion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let store = ProfileStore::new(home).expect("store");
        store
            .upsert(
                json!({"id":"python-secret","name":"Python Secret"})
                    .as_object()
                    .cloned()
                    .expect("profile"),
                None,
            )
            .expect("upsert");

        let keys = store
            .update_secrets(
                "python-secret",
                json!({
                    "BOOL":true,
                    "NONE":null,
                    "NUMBER":42,
                    "OBJECT":{"nested":"value"}
                })
                .as_object()
                .expect("secrets"),
                &[],
                false,
            )
            .expect("update secrets");

        assert_eq!(keys, vec!["BOOL", "NONE", "NUMBER", "OBJECT"]);
        assert_eq!(
            store.secret_values("python-secret").expect("secrets"),
            BTreeMap::from([
                ("BOOL".into(), "True".into()),
                ("NONE".into(), "None".into()),
                ("NUMBER".into(), "42".into()),
                ("OBJECT".into(), "{'nested': 'value'}".into()),
            ])
        );
    }

    #[test]
    fn capability_default_cleanup_updates_every_profile() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let store = ProfileStore::new(home).expect("store");
        for id in ["one", "two"] {
            store
                .upsert(
                    json!({
                        "id":id,"name":id,
                        "capability_defaults":{"autoload_capabilities":["skill:remove","skill:keep"]}
                    })
                    .as_object()
                    .cloned()
                    .expect("profile"),
                    None,
                )
                .expect("upsert");
        }

        assert_eq!(
            store
                .remove_capability_default("skill:remove")
                .expect("cleanup"),
            2
        );
        for id in ["one", "two"] {
            assert_eq!(
                store.get(id).expect("get").expect("profile")["capability_defaults"]["autoload_capabilities"],
                json!(["skill:keep"])
            );
        }
    }
}

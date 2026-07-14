use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use uuid::Uuid;

use crate::fs::{read_json, write_json};
use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone)]
pub struct ProfileStore {
    home: HomeLayout,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfileDoc {
    #[serde(default)]
    profiles: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretDoc {
    #[serde(default)]
    profiles: BTreeMap<String, BTreeMap<String, String>>,
}

impl ProfileStore {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        Ok(Self { home })
    }

    pub fn list(&self) -> io::Result<Vec<Value>> {
        let mut profiles = self.load()?.profiles.into_values().collect::<Vec<_>>();
        for profile in &mut profiles {
            profile["usage_count"] = json!(self.usage(profile["id"].as_str().unwrap_or(""))?.len());
        }
        profiles.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(profiles)
    }

    pub fn get(&self, profile_id: &str) -> io::Result<Option<Value>> {
        validate_id(profile_id)?;
        Ok(self.load()?.profiles.get(profile_id).cloned())
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
            .unwrap_or_else(|| format!("ap_{}", &Uuid::new_v4().simple().to_string()[..16]));
        validate_id(&id)?;
        let current = doc.profiles.get(&id);
        let revision = current
            .and_then(|value| value["revision"].as_u64())
            .unwrap_or(0);
        if expected.is_some_and(|expected| expected != revision) {
            return Err(io::Error::other(format!(
                "revision conflict: expected {}, current {revision}",
                expected.unwrap_or_default()
            )));
        }
        let now = utc_now();
        profile.insert("id".into(), json!(id));
        profile.entry("name").or_insert_with(|| json!(id));
        profile.entry("scope").or_insert_with(|| json!("global"));
        profile.entry("owner_id").or_insert_with(|| json!(""));
        profile.entry("runtime").or_insert_with(|| json!("codex"));
        profile.entry("runner").or_insert_with(|| json!("pty"));
        profile.entry("command").or_insert_with(|| json!([]));
        profile.entry("submit").or_insert_with(|| json!("enter"));
        profile.entry("env").or_insert_with(|| json!({}));
        profile.insert(
            "created_at".into(),
            current
                .and_then(|value| value["created_at"].as_str())
                .map_or_else(|| json!(now), |value| json!(value)),
        );
        profile.insert("updated_at".into(), json!(utc_now()));
        profile.insert("revision".into(), json!(revision + 1));
        let result = Value::Object(profile);
        doc.profiles.insert(id, result.clone());
        self.save(&doc)?;
        Ok(result)
    }

    pub fn delete(&self, profile_id: &str, force_detach: bool) -> io::Result<(bool, Vec<Value>)> {
        validate_id(profile_id)?;
        let usage = self.usage(profile_id)?;
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
                        actor.profile_revision_applied = 0;
                    }
                    Ok(())
                })?;
            }
        }
        let mut doc = self.load()?;
        let deleted = doc.profiles.remove(profile_id).is_some();
        self.save(&doc)?;
        let mut secrets = self.load_secrets()?;
        secrets.profiles.remove(profile_id);
        self.save_secrets(&secrets)?;
        Ok((deleted, usage))
    }

    pub fn usage(&self, profile_id: &str) -> io::Result<Vec<Value>> {
        let groups = GroupStore::new(self.home.clone())?;
        let mut usage = Vec::new();
        for meta in groups.list()? {
            if let Ok(group) = groups.load(&meta.group_id) {
                for actor in group
                    .actors
                    .iter()
                    .filter(|actor| actor.profile_id == profile_id)
                {
                    usage.push(json!({"group_id":group.group_id,"group_title":group.title,"actor_id":actor.id,"actor_title":actor.title}));
                }
            }
        }
        Ok(usage)
    }

    pub fn secret_keys(&self, profile_id: &str) -> io::Result<Vec<String>> {
        validate_id(profile_id)?;
        Ok(self
            .load_secrets()?
            .profiles
            .get(profile_id)
            .into_iter()
            .flat_map(|values| values.keys())
            .cloned()
            .collect())
    }
    pub fn secret_values(&self, profile_id: &str) -> io::Result<BTreeMap<String, String>> {
        validate_id(profile_id)?;
        Ok(self
            .load_secrets()?
            .profiles
            .get(profile_id)
            .cloned()
            .unwrap_or_default())
    }
    pub fn update_secrets(
        &self,
        profile_id: &str,
        set: &Map<String, Value>,
        unset: &[Value],
        clear: bool,
    ) -> io::Result<Vec<String>> {
        validate_id(profile_id)?;
        if self.get(profile_id)?.is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "profile not found"));
        }
        let mut doc = self.load_secrets()?;
        let values = doc.profiles.entry(profile_id.into()).or_default();
        if clear {
            values.clear();
        }
        for (key, value) in set {
            values.insert(
                key.clone(),
                value
                    .as_str()
                    .ok_or_else(|| io::Error::other("secret values must be strings"))?
                    .into(),
            );
        }
        for key in unset.iter().filter_map(Value::as_str) {
            values.remove(key);
        }
        let keys = values.keys().cloned().collect();
        self.save_secrets(&doc)?;
        Ok(keys)
    }
    pub fn replace_secrets(
        &self,
        profile_id: &str,
        values: BTreeMap<String, String>,
    ) -> io::Result<Vec<String>> {
        let mut doc = self.load_secrets()?;
        let keys = values.keys().cloned().collect();
        doc.profiles.insert(profile_id.into(), values);
        self.save_secrets(&doc)?;
        Ok(keys)
    }

    fn load(&self) -> io::Result<ProfileDoc> {
        let path = self.home.root().join("profiles.json");
        if path.exists() {
            read_json(&path)
        } else {
            Ok(ProfileDoc::default())
        }
    }
    fn save(&self, value: &ProfileDoc) -> io::Result<()> {
        write_json(&self.home.root().join("profiles.json"), value)
    }
    fn load_secrets(&self) -> io::Result<SecretDoc> {
        let path = self.home.root().join("profile-secrets.json");
        if path.exists() {
            read_json(&path)
        } else {
            Ok(SecretDoc::default())
        }
    }
    fn save_secrets(&self, value: &SecretDoc) -> io::Result<()> {
        write_json(&self.home.root().join("profile-secrets.json"), value)
    }
}

fn validate_id(value: &str) -> io::Result<()> {
    (!value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(())
    .ok_or_else(|| io::Error::other("invalid profile_id"))
}

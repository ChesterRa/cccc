use cccc_contracts::{Actor, GroupState, utc_now};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use uuid::Uuid;

use crate::fs::{read_yaml, with_exclusive_lock, write_yaml};
use crate::home::HomeLayout;
use crate::registry::{GroupMeta, Registry};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub scope_key: String,
    pub url: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub git_remote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupDoc {
    pub v: u8,
    pub group_id: String,
    pub title: String,
    #[serde(default)]
    pub topic: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub state: GroupState,
    #[serde(default)]
    pub active_scope_key: String,
    #[serde(default)]
    pub scopes: Vec<Scope>,
    #[serde(default)]
    pub actors: Vec<Actor>,
    #[serde(default)]
    pub automation: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct GroupStore {
    home: HomeLayout,
}

impl GroupStore {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        Ok(Self { home })
    }

    pub fn create(&self, title: &str, topic: &str) -> io::Result<GroupDoc> {
        let now = utc_now();
        let group = GroupDoc {
            v: 1,
            group_id: format!("g_{}", &Uuid::new_v4().simple().to_string()[..12]),
            title: normalized_title(title),
            topic: topic.trim().to_owned(),
            created_at: now.clone(),
            updated_at: now,
            running: false,
            state: GroupState::Active,
            active_scope_key: String::new(),
            scopes: Vec::new(),
            actors: Vec::new(),
            automation: Map::new(),
            extra: Map::new(),
        };
        let dir = self.group_dir(&group.group_id)?;
        for child in ["context", "scopes", "state", "state/blobs"] {
            fs::create_dir_all(dir.join(child))?;
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("ledger.jsonl"))?;
        self.save(&group)?;
        let mut registry = Registry::load(&self.home)?;
        registry.insert(GroupMeta {
            group_id: group.group_id.clone(),
            title: group.title.clone(),
            topic: group.topic.clone(),
            path: dir.to_string_lossy().into_owned(),
            default_scope_key: String::new(),
            created_at: group.created_at.clone(),
            updated_at: group.updated_at.clone(),
        })?;
        Ok(group)
    }

    pub fn load(&self, group_id: &str) -> io::Result<GroupDoc> {
        read_yaml(&self.group_dir(group_id)?.join("group.yaml"))
    }

    pub fn save(&self, group: &GroupDoc) -> io::Result<()> {
        validate_group_id(&group.group_id)?;
        with_exclusive_lock(&self.group_lock_path(&group.group_id)?, || {
            self.save_unlocked(group)
        })
    }

    fn save_unlocked(&self, group: &GroupDoc) -> io::Result<()> {
        let mut stored = group.clone();
        stored.updated_at = utc_now();
        write_yaml(
            &self.group_dir(&stored.group_id)?.join("group.yaml"),
            &stored,
        )
    }

    pub fn list(&self) -> io::Result<Vec<GroupMeta>> {
        Ok(Registry::load(&self.home)?.groups.into_values().collect())
    }

    pub fn update(
        &self,
        group_id: &str,
        title: Option<&str>,
        topic: Option<&str>,
    ) -> io::Result<GroupDoc> {
        let group = with_exclusive_lock(&self.group_lock_path(group_id)?, || {
            let mut group = self.load(group_id)?;
            if let Some(value) = title {
                group.title = normalized_title(value);
            }
            if let Some(value) = topic {
                group.topic = value.trim().to_owned();
            }
            group.updated_at = utc_now();
            self.save_unlocked(&group)?;
            Ok(group)
        })?;
        let mut registry = Registry::load(&self.home)?;
        if let Some(meta) = registry.groups.get_mut(group_id) {
            meta.title.clone_from(&group.title);
            meta.topic.clone_from(&group.topic);
            meta.updated_at.clone_from(&group.updated_at);
        }
        registry.save()?;
        Ok(group)
    }

    pub fn delete(&self, group_id: &str) -> io::Result<bool> {
        let dir = self.group_dir(group_id)?;
        let existed = dir.exists();
        if existed {
            fs::remove_dir_all(dir)?;
        }
        Registry::load(&self.home)?.remove(group_id)?;
        Ok(existed)
    }

    pub fn ledger_path(&self, group_id: &str) -> io::Result<std::path::PathBuf> {
        Ok(self.group_dir(group_id)?.join("ledger.jsonl"))
    }

    pub fn mutate<T>(
        &self,
        group_id: &str,
        change: impl FnOnce(&mut GroupDoc) -> io::Result<T>,
    ) -> io::Result<T> {
        with_exclusive_lock(&self.group_lock_path(group_id)?, || {
            let mut group = self.load(group_id)?;
            let result = change(&mut group)?;
            self.save_unlocked(&group)?;
            Ok(result)
        })
    }

    pub fn state_dir(&self, group_id: &str) -> io::Result<std::path::PathBuf> {
        Ok(self.group_dir(group_id)?.join("state"))
    }

    #[must_use]
    pub fn home(&self) -> &HomeLayout {
        &self.home
    }

    pub fn group_dir(&self, group_id: &str) -> io::Result<std::path::PathBuf> {
        validate_group_id(group_id)?;
        Ok(self.home.groups_dir().join(group_id))
    }

    fn group_lock_path(&self, group_id: &str) -> io::Result<std::path::PathBuf> {
        Ok(self.group_dir(group_id)?.join("group.yaml.lock"))
    }

    pub fn import(&self, mut group: GroupDoc) -> io::Result<GroupDoc> {
        validate_group_id(&group.group_id)?;
        let dir = self.group_dir(&group.group_id)?;
        if dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "group already exists",
            ));
        }
        for child in ["context", "scopes", "state", "state/blobs"] {
            fs::create_dir_all(dir.join(child))?;
        }
        group.updated_at = utc_now();
        write_yaml(&dir.join("group.yaml"), &group)?;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("ledger.jsonl"))?;
        let mut registry = Registry::load(&self.home)?;
        registry.insert(GroupMeta {
            group_id: group.group_id.clone(),
            title: group.title.clone(),
            topic: group.topic.clone(),
            path: dir.to_string_lossy().into_owned(),
            default_scope_key: group.active_scope_key.clone(),
            created_at: group.created_at.clone(),
            updated_at: group.updated_at.clone(),
        })?;
        Ok(group)
    }
}

fn validate_group_id(value: &str) -> io::Result<()> {
    let valid = value.starts_with("g_")
        && value.len() >= 5
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(io::Error::other("invalid group_id"))
    }
}

fn normalized_title(value: &str) -> String {
    let title = value.trim();
    if title.is_empty() {
        "working-group".into()
    } else {
        title.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_mutations_do_not_overwrite_each_other() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("concurrency", "").expect("group");
        let barrier = Arc::new(Barrier::new(16));
        let handles = (0..16)
            .map(|_| {
                let store = store.clone();
                let group_id = group.group_id.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .mutate(&group_id, |group| {
                            let count = group
                                .extra
                                .get("concurrent_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(0);
                            group
                                .extra
                                .insert("concurrent_count".into(), (count + 1).into());
                            Ok(())
                        })
                        .expect("mutate");
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().expect("join");
        }
        assert_eq!(
            store.load(&group.group_id).expect("load").extra["concurrent_count"],
            16
        );
    }
}

use cccc_contracts::{Actor, GroupState, utc_now};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use uuid::Uuid;

use crate::fs::{read_yaml, with_exclusive_lock, write_yaml, write_yaml_committed};
use crate::home::HomeLayout;
use crate::registry::{GroupMeta, Registry};

pub const AUTOMATION_TIMING_KEYS: &[&str] = &[
    "actor_idle_timeout_seconds",
    "keepalive_delay_seconds",
    "keepalive_max_per_actor",
    "silence_timeout_seconds",
    "help_nudge_interval_seconds",
    "help_nudge_min_messages",
];

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

/// Read automation timing from the canonical automation document, with a
/// bounded fallback for groups written by the former native settings layout.
pub fn automation_timing_value<'a>(group: &'a GroupDoc, key: &str) -> Option<&'a Value> {
    group.automation.get(key).or_else(|| {
        group
            .extra
            .get("settings")
            .and_then(Value::as_object)
            .and_then(|settings| settings.get(key))
    })
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
        self.create_with_registry(title, topic, |home, meta| {
            Registry::mutate(home, |registry| {
                registry.groups.insert(meta.group_id.clone(), meta);
                Ok(())
            })
        })
    }

    fn create_with_registry(
        &self,
        title: &str,
        topic: &str,
        register: impl FnOnce(&HomeLayout, GroupMeta) -> io::Result<()>,
    ) -> io::Result<GroupDoc> {
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
        let result = (|| {
            for child in ["context", "scopes", "state", "state/blobs"] {
                fs::create_dir_all(dir.join(child))?;
            }
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("ledger.jsonl"))?;
            self.save(&group)?;
            let meta = GroupMeta {
                group_id: group.group_id.clone(),
                title: group.title.clone(),
                topic: group.topic.clone(),
                path: dir.to_string_lossy().into_owned(),
                default_scope_key: String::new(),
                created_at: group.created_at.clone(),
                updated_at: group.updated_at.clone(),
            };
            register(&self.home, meta)?;
            Ok(group)
        })();
        result.map_err(|error| match fs::remove_dir_all(&dir) {
            Ok(()) => error,
            Err(rollback) if rollback.kind() == io::ErrorKind::NotFound => error,
            Err(rollback) => io::Error::other(format!(
                "{error}; rollback_failed: could not remove {}: {rollback}",
                dir.display()
            )),
        })
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
        self.save_unlocked_doc(group).map(|_| ())
    }

    fn save_unlocked_doc(&self, group: &GroupDoc) -> io::Result<GroupDoc> {
        let mut stored = group.clone();
        stored.updated_at = utc_now();
        write_yaml_committed(
            &self.group_dir(&stored.group_id)?.join("group.yaml"),
            &stored,
        )?;
        Ok(stored)
    }

    fn restore_unlocked(&self, group: &GroupDoc) -> io::Result<()> {
        write_yaml_committed(&self.group_dir(&group.group_id)?.join("group.yaml"), group)
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
        Registry::mutate(&self.home, |registry| {
            if let Some(meta) = registry.groups.get_mut(group_id) {
                meta.title.clone_from(&group.title);
                meta.topic.clone_from(&group.topic);
                meta.updated_at.clone_from(&group.updated_at);
            }
            Ok(())
        })?;
        Ok(group)
    }

    pub fn delete(&self, group_id: &str) -> io::Result<bool> {
        crate::group_delete::delete(self, group_id)
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

    pub fn mutate_with_rollback<T>(
        &self,
        group_id: &str,
        change: impl FnOnce(&mut GroupDoc) -> io::Result<T>,
        side_effect: impl FnOnce(&T) -> io::Result<()>,
    ) -> io::Result<T> {
        with_exclusive_lock(&self.group_lock_path(group_id)?, || {
            let before = self.load(group_id)?;
            let mut group = before.clone();
            let result = change(&mut group)?;
            let written = self.save_unlocked_doc(&group)?;
            if let Err(error) = side_effect(&result) {
                return match self.load(group_id) {
                    Ok(current) if current == written => match self.restore_unlocked(&before) {
                        Ok(()) => Err(error),
                        Err(rollback) => Err(io::Error::other(format!(
                            "{error}; rollback_failed: could not restore group: {rollback}"
                        ))),
                    },
                    Ok(_) => Err(io::Error::other(format!(
                        "{error}; rollback_skipped: group changed concurrently"
                    ))),
                    Err(rollback) => Err(io::Error::other(format!(
                        "{error}; rollback_failed: could not verify current group: {rollback}"
                    ))),
                };
            }
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
        let meta = GroupMeta {
            group_id: group.group_id.clone(),
            title: group.title.clone(),
            topic: group.topic.clone(),
            path: dir.to_string_lossy().into_owned(),
            default_scope_key: group.active_scope_key.clone(),
            created_at: group.created_at.clone(),
            updated_at: group.updated_at.clone(),
        };
        Registry::mutate(&self.home, |registry| {
            registry.groups.insert(meta.group_id.clone(), meta);
            Ok(())
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

    #[test]
    fn concurrent_group_creates_keep_every_registry_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let barrier = Arc::new(Barrier::new(16));
        let handles = (0..16)
            .map(|index| {
                let store = store.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.create(&format!("group-{index}"), "").expect("create")
                })
            })
            .collect::<Vec<_>>();
        let group_ids = handles
            .into_iter()
            .map(|handle| handle.join().expect("join").group_id)
            .collect::<std::collections::BTreeSet<_>>();
        let listed = store
            .list()
            .expect("list")
            .into_iter()
            .map(|group| group.group_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(listed, group_ids);
    }

    #[test]
    fn committed_registry_error_does_not_delete_the_created_group_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create_with_registry("committed", "", |home, meta| {
            Registry::mutate_with_writer(
                home,
                |registry| {
                    registry.groups.insert(meta.group_id.clone(), meta);
                    Ok(())
                },
                |path, value| {
                    crate::fs::write_json(path, value)?;
                    Err(io::Error::other("injected sync_dir failure"))
                },
            )
        });
        assert!(created.is_ok(), "{created:?}");
        let group = created.expect("group");
        assert!(store.group_dir(&group.group_id).expect("dir").is_dir());
        assert!(
            Registry::load(&home)
                .expect("registry")
                .groups
                .contains_key(&group.group_id)
        );
    }
}

use cccc_contracts::{Actor, GroupState, utc_now};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::Path;
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
    /// Resource authorization generation. Import creates a fresh generation even when preserving the ID.
    #[serde(default)]
    pub generation: String,
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
            generation: Uuid::new_v4().to_string(),
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
        let mut group: GroupDoc = read_yaml(&self.group_dir(group_id)?.join("group.yaml"))?;
        for actor in &mut group.actors {
            actor.normalize_runtime_constraints();
        }
        Ok(group)
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

    pub fn import(&self, group: GroupDoc) -> io::Result<GroupDoc> {
        self.import_prepared(group, |_, _| Ok(()))
    }

    /// Construct an imported Group before making its document visible at the
    /// final path. Readers can address a Group directly without consulting the
    /// registry, so the document itself is the publication boundary.
    pub(crate) fn import_prepared(
        &self,
        group: GroupDoc,
        prepare: impl FnOnce(&Path, &GroupDoc) -> io::Result<()>,
    ) -> io::Result<GroupDoc> {
        self.import_prepared_with_registry(group, prepare, |registry, meta| {
            registry.groups.insert(meta.group_id.clone(), meta);
            Ok(())
        })
    }

    fn import_prepared_with_registry(
        &self,
        group: GroupDoc,
        prepare: impl FnOnce(&Path, &GroupDoc) -> io::Result<()>,
        register: impl FnOnce(&mut Registry, GroupMeta) -> io::Result<()>,
    ) -> io::Result<GroupDoc> {
        self.import_prepared_with_install(
            group,
            prepare,
            |stage, dir| move_import_contents(stage, dir, |from, to| fs::rename(from, to)),
            register,
        )
    }

    fn import_prepared_with_install(
        &self,
        mut group: GroupDoc,
        prepare: impl FnOnce(&Path, &GroupDoc) -> io::Result<()>,
        install: impl FnOnce(&Path, &Path) -> io::Result<()>,
        register: impl FnOnce(&mut Registry, GroupMeta) -> io::Result<()>,
    ) -> io::Result<GroupDoc> {
        validate_group_id(&group.group_id)?;
        let dir = self.group_dir(&group.group_id)?;
        if dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "group already exists",
            ));
        }
        if Registry::load(&self.home)?
            .groups
            .contains_key(&group.group_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "group already registered",
            ));
        }
        let stage = tempfile::Builder::new()
            .prefix(".importing-")
            .tempdir_in(self.home.groups_dir())?;
        for child in ["context", "scopes", "state", "state/blobs"] {
            fs::create_dir_all(stage.path().join(child))?;
        }
        group.generation = Uuid::new_v4().to_string();
        group.updated_at = utc_now();
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stage.path().join("ledger.jsonl"))?;
        prepare(stage.path(), &group)?;
        write_yaml(&stage.path().join("group.yaml"), &group)?;

        // `rename` may replace an empty destination directory on Unix. Reserve
        // the name with create_dir instead so concurrent imports cannot replace
        // one another. Move the document last: until then load() must fail.
        fs::create_dir(&dir)?;
        let move_contents = install(stage.path(), &dir);
        // Another process may still hold this id from an earlier Group and
        // write into the reservation, even before group.yaml is published.
        // Never delete its possible writes as part of a failed import.
        let retain_unpublished = |error: io::Error| {
            io::Error::new(
                error.kind(),
                format!(
                    "{error}; incomplete import {} retained at {}",
                    group.group_id,
                    dir.display()
                ),
            )
        };
        if let Err(error) = move_contents {
            return Err(retain_unpublished(error));
        }

        let meta = GroupMeta {
            group_id: group.group_id.clone(),
            title: group.title.clone(),
            topic: group.topic.clone(),
            path: dir.to_string_lossy().into_owned(),
            default_scope_key: group.active_scope_key.clone(),
            created_at: group.created_at.clone(),
            updated_at: group.updated_at.clone(),
        };
        let mut published = false;
        let result = Registry::mutate(&self.home, |registry| {
            if registry.groups.contains_key(&meta.group_id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "group already registered",
                ));
            }
            // Direct callers can load the Group as soon as group.yaml exists.
            // Keep publication and registry insertion in the same short lock:
            // a concurrent update or deletion then follows this registration.
            fs::rename(stage.path().join("group.yaml"), dir.join("group.yaml"))?;
            published = true;
            register(registry, meta)
        });
        if let Err(error) = result {
            if !published {
                return Err(retain_unpublished(error));
            }
            // Once group.yaml exists, callers can have successfully changed
            // the Group even if registry registration fails. Never roll those
            // writes back by deleting the directory.
            return Err(io::Error::other(format!(
                "{error}; imported group {} is complete but could not be registered; data retained at {}",
                group.group_id,
                dir.display()
            )));
        }
        Ok(group)
    }
}

/// A reader of a recently deleted Group may create state/ledger as soon as the
/// id is reserved. Reuse that directory and hold its usual writer lock while
/// installing the contents, so readers cannot observe a partial ledger.
/// Keep the document in the stage until registry publication succeeds.
fn move_import_contents(
    stage: &Path,
    dir: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let _ledger_lock = crate::ledger::acquire_writer_lock(&dir.join("ledger.jsonl"))?;
    let state = stage.join("state");
    let dest_state = dir.join("state");
    for entry in fs::read_dir(&dest_state)? {
        if entry?.file_name() != "ledger" {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "import state already exists",
            ));
        }
    }
    for entry in fs::read_dir(dest_state.join("ledger"))? {
        if entry?.file_name() != "ledger.lock" {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "import ledger state already exists",
            ));
        }
    }
    for entry in fs::read_dir(&state)? {
        let entry = entry?;
        if entry.file_name() == "ledger" {
            if !entry.file_type()?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "imported ledger state is not a directory",
                ));
            }
            for item in fs::read_dir(entry.path())? {
                let item = item?;
                move_import_entry(
                    &item.path(),
                    &dest_state.join("ledger").join(item.file_name()),
                    &mut rename,
                )?;
            }
        } else {
            move_import_entry(
                &entry.path(),
                &dest_state.join(entry.file_name()),
                &mut rename,
            )?;
        }
    }
    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        if entry.file_name() != "group.yaml"
            && entry.file_name() != "ledger.jsonl"
            && entry.file_name() != "state"
        {
            move_import_entry(&entry.path(), &dir.join(entry.file_name()), &mut rename)?;
        }
    }
    move_import_entry(
        &stage.join("ledger.jsonl"),
        &dir.join("ledger.jsonl"),
        &mut rename,
    )
}

fn move_import_entry(
    from: &Path,
    to: &Path,
    rename: &mut impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    match fs::symlink_metadata(to) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "import target already exists",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => rename(from, to),
        Err(error) => Err(error),
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
    fn import_holds_the_ledger_lock_while_installing_contents() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = |name: &str| {
            let stage = temp.path().join(format!("{name}-stage"));
            let dir = temp.path().join(format!("{name}-dest"));
            fs::create_dir_all(stage.join("state")).expect("staged state");
            fs::create_dir_all(stage.join("context")).expect("staged context");
            fs::create_dir(&dir).expect("target");
            fs::write(stage.join("ledger.jsonl"), b"").expect("ledger");
            fs::write(stage.join("group.yaml"), b"document").expect("document");
            (stage, dir)
        };

        // The old order can expose the ledger while state is still staged.
        let (old_stage, old_dir) = fixture("old");
        fs::rename(old_stage.join("ledger.jsonl"), old_dir.join("ledger.jsonl"))
            .expect("expose ledger");
        crate::ledger::LedgerFollower::at_end(&old_dir.join("ledger.jsonl"))
            .expect("observer creates its lock");
        assert!(fs::rename(old_stage.join("state"), old_dir.join("state")).is_err());

        let (stage, dir) = fixture("fixed");
        let mut observed = false;
        move_import_contents(&stage, &dir, |from, to| {
            fs::rename(from, to)?;
            if to.file_name().is_some_and(|name| name == "ledger.jsonl") {
                assert!(!dir.join("group.yaml").exists());
                let other = fs::File::open(dir.join("state/ledger/ledger.lock"))?;
                assert!(
                    fs2::FileExt::try_lock_shared(&other).is_err(),
                    "import must hold the ledger lock until contents are ready"
                );
                observed = true;
            }
            Ok(())
        })
        .expect("observer cannot prevent the import");
        assert!(observed);
        assert!(dir.join("state/ledger").is_dir());
        assert!(dir.join("context").is_dir());
        assert!(stage.join("group.yaml").is_file());
    }

    #[test]
    fn old_readers_and_import_reuse_the_same_ledger_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stage = temp.path().join("stage");
        let dir = temp.path().join("g_reimported");
        fs::create_dir_all(stage.join("state")).expect("staged state");
        fs::create_dir_all(stage.join("state/ledger/segments")).expect("staged segments");
        fs::write(stage.join("state/ledger/segments/ledger.0001.jsonl"), b"").expect("segment");
        fs::write(stage.join("state/note.txt"), b"retained").expect("state file");
        fs::create_dir(&dir).expect("reserved target");
        fs::write(stage.join("ledger.jsonl"), b"").expect("staged ledger");
        fs::write(stage.join("group.yaml"), b"document").expect("staged document");

        let path = dir.join("ledger.jsonl");
        // A previously subscribed reader is still polling this id after its
        // old Group was deleted, before the imported contents are moved.
        let (mut follower, cursor) =
            crate::ledger::LedgerFollower::at_end(&path).expect("empty unpublished ledger");
        assert!(cursor.is_none());
        assert!(follower.poll(&path).expect("poll unpublished").is_empty());
        assert!(
            crate::ledger::tail(&path, 1)
                .expect("tail unpublished")
                .is_empty()
        );
        let lock = dir.join("state/ledger/ledger.lock");
        fs::write(&lock, b"existing lock").expect("mark existing lock");

        move_import_contents(&stage, &dir, |from, to| fs::rename(from, to))
            .expect("the reader cannot prevent publication");
        assert_eq!(fs::read(&lock).expect("lock retained"), b"existing lock");
        assert!(
            dir.join("state/ledger/segments/ledger.0001.jsonl")
                .is_file()
        );
        assert_eq!(
            fs::read(dir.join("state/note.txt")).expect("state retained"),
            b"retained"
        );
        let event = cccc_contracts::Event::new("chat.message", "g_reimported");
        crate::ledger::append(&path, &event).expect("write after contents moved");
        assert_eq!(follower.poll(&path).expect("new event"), vec![event]);
    }

    #[test]
    fn import_refuses_unexpected_existing_state_without_overwriting_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        for existing in ["state/note.txt", "state/ledger/index.sqlite3"] {
            let stage = tempfile::tempdir_in(temp.path()).expect("stage");
            let dir = tempfile::tempdir_in(temp.path()).expect("target");
            fs::create_dir_all(stage.path().join("state")).expect("staged state");
            fs::write(stage.path().join("ledger.jsonl"), b"").expect("ledger");
            fs::create_dir_all(dir.path().join(existing).parent().expect("parent")).expect("state");
            fs::write(dir.path().join(existing), b"existing").expect("old state");
            let error =
                move_import_contents(stage.path(), dir.path(), |from, to| fs::rename(from, to))
                    .expect_err("unexpected state must be rejected");
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(
                fs::read(dir.path().join(existing)).expect("old state retained"),
                b"existing"
            );
            assert!(stage.path().join("ledger.jsonl").is_file());
        }
    }

    #[test]
    fn a_failed_import_keeps_state_written_after_reserving_its_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("source", "").expect("source");
        group.group_id = "g_import_conflict".into();
        let dir = store.group_dir(&group.group_id).expect("path");
        let error = store
            .import_prepared_with_install(
                group,
                |_, _| Ok(()),
                |stage, target| {
                    fs::create_dir_all(target.join("state"))?;
                    fs::write(target.join("state/external.txt"), b"concurrent write")?;
                    move_import_contents(stage, target, |from, to| fs::rename(from, to))
                },
                |registry, meta| {
                    registry.groups.insert(meta.group_id.clone(), meta);
                    Ok(())
                },
            )
            .expect_err("conflict");
        assert_eq!(
            fs::read(dir.join("state/external.txt")).expect("concurrent write retained"),
            b"concurrent write"
        );
        assert!(
            error.to_string().contains(&dir.display().to_string()),
            "error must name retained directory: {error}"
        );
        assert!(!dir.join("group.yaml").exists());
        assert!(
            store
                .list()
                .expect("registry")
                .iter()
                .all(|meta| meta.group_id != "g_import_conflict")
        );
    }

    #[test]
    fn an_unpublished_registry_conflict_keeps_a_concurrent_ledger_write() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("source", "").expect("source");
        let mut occupied = store
            .list()
            .expect("registry")
            .into_iter()
            .find(|meta| meta.group_id == group.group_id)
            .expect("metadata");
        group.group_id = "g_import_late".into();
        occupied.group_id.clone_from(&group.group_id);
        let dir = store.group_dir(&group.group_id).expect("path");
        let event = cccc_contracts::Event::new("chat.message", "g_import_late");
        let error = store
            .import_prepared_with_install(
                group,
                |_, _| Ok(()),
                |stage, target| {
                    move_import_contents(stage, target, |from, to| fs::rename(from, to))?;
                    // The normal ledger lock has been released, but the Group has not
                    // yet been published or registered.
                    crate::ledger::append(&target.join("ledger.jsonl"), &event)?;
                    Registry::mutate(&home, |registry| {
                        registry.groups.insert(occupied.group_id.clone(), occupied);
                        Ok(())
                    })
                },
                |registry, meta| {
                    registry.groups.insert(meta.group_id.clone(), meta);
                    Ok(())
                },
            )
            .expect_err("registry conflict");
        assert!(
            fs::read_to_string(dir.join("ledger.jsonl"))
                .expect("ledger retained")
                .contains(&event.id)
        );
        assert!(
            error.to_string().contains(&dir.display().to_string()),
            "error must name retained directory: {error}"
        );
        assert!(!dir.join("group.yaml").exists());
    }

    #[test]
    fn import_is_invisible_until_all_contents_are_ready() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut source = store.create("source", "").expect("source");
        source.group_id = "g_invisible".into();
        let id = source.group_id.clone();
        let (prepared_tx, prepared_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let worker = store.clone();
        let import = std::thread::spawn(move || {
            worker.import_prepared(source, |dir, _| {
                fs::write(dir.join("ledger.jsonl"), b"ready history\n")?;
                fs::write(dir.join("context/fixture"), b"ready context")?;
                prepared_tx.send(()).expect("notify prepared");
                finish_rx.recv().expect("finish preparation");
                Ok(())
            })
        });
        prepared_rx.recv().expect("wait for preparation");
        assert!(!store.group_dir(&id).expect("path").exists());
        assert!(store.load(&id).is_err());
        assert!(
            !store
                .list()
                .expect("registry")
                .iter()
                .any(|meta| meta.group_id == id)
        );
        finish_tx.send(()).expect("release preparation");
        import.join().expect("join").expect("import");
        let dir = store.group_dir(&id).expect("path");
        assert_eq!(
            fs::read(dir.join("ledger.jsonl")).expect("ledger"),
            b"ready history\n"
        );
        assert_eq!(
            fs::read(dir.join("context/fixture")).expect("context"),
            b"ready context"
        );
        assert_eq!(store.load(&id).expect("group").title, "source");
        assert!(
            store
                .list()
                .expect("registry")
                .iter()
                .any(|meta| meta.group_id == id)
        );
    }

    #[test]
    fn registry_failure_after_publication_preserves_concurrent_writes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("source", "").expect("source");
        group.group_id = "g_published".into();
        let id = group.group_id.clone();
        let accepted = cccc_contracts::Event::new("chat.message", &id);
        let error = store
            .import_prepared_with_registry(
                group,
                |_, _| Ok(()),
                |_, meta| {
                    // Direct readers can see the Group before its registry update.
                    assert!(store.load(&meta.group_id).is_ok());
                    crate::ledger::append(&store.ledger_path(&meta.group_id)?, &accepted)?;
                    Err(io::Error::other("injected registry failure"))
                },
            )
            .expect_err("registration failure");
        assert!(error.to_string().contains("data retained"));
        assert!(store.load(&id).is_ok());
        assert_eq!(
            crate::ledger::tail(&store.ledger_path(&id).expect("path"), 1).expect("ledger")[0].id,
            accepted.id
        );
    }

    #[test]
    fn a_group_update_at_import_publication_is_reflected_in_the_registry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("source", "").expect("source");
        group.group_id = "g_concurrent_update".into();
        let id = group.group_id.clone();
        let mut updater = None;
        store
            .import_prepared_with_registry(
                group,
                |_, _| Ok(()),
                |registry, meta| {
                    let worker = store.clone();
                    let id = meta.group_id.clone();
                    updater = Some(std::thread::spawn(move || {
                        worker.update(&id, Some("new title"), None)
                    }));
                    // The update writes group.yaml before waiting for this same
                    // registry lock. Insert the imported metadata while it waits.
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while store.load(&meta.group_id)?.title != "new title" {
                        assert!(std::time::Instant::now() < deadline, "update did not start");
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    registry.groups.insert(meta.group_id.clone(), meta);
                    Ok(())
                },
            )
            .expect("import");
        updater
            .expect("updater")
            .join()
            .expect("join")
            .expect("update");

        assert_eq!(store.load(&id).expect("group").title, "new title");
        assert_eq!(
            store
                .list()
                .expect("registry")
                .into_iter()
                .find(|meta| meta.group_id == id)
                .expect("entry")
                .title,
            "new title"
        );
    }

    #[test]
    fn a_group_deleted_at_import_publication_does_not_leave_a_registry_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("source", "").expect("source");
        group.group_id = "g_concurrent_delete".into();
        let id = group.group_id.clone();
        let dir = store.group_dir(&id).expect("path");
        let mut deleter = None;
        store
            .import_prepared_with_registry(
                group,
                |_, _| Ok(()),
                |registry, meta| {
                    let worker = store.clone();
                    let id = meta.group_id.clone();
                    deleter = Some(std::thread::spawn(move || worker.delete(&id)));
                    // The directory is moved aside before deletion waits for the
                    // registry lock to remove the entry.
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while dir.exists() {
                        assert!(std::time::Instant::now() < deadline, "delete did not start");
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    registry.groups.insert(meta.group_id.clone(), meta);
                    Ok(())
                },
            )
            .expect("import");
        assert!(
            deleter
                .expect("deleter")
                .join()
                .expect("join")
                .expect("delete")
        );
        assert!(!dir.exists());
        assert!(
            !store
                .list()
                .expect("registry")
                .iter()
                .any(|meta| meta.group_id == id)
        );
    }

    #[test]
    fn concurrent_imports_cannot_replace_the_same_group_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("source", "").expect("source");
        group.group_id = "g_collision".into();
        let barrier = Arc::new(Barrier::new(2));
        let handles = ["first", "second"].map(|name| {
            let store = store.clone();
            let barrier = Arc::clone(&barrier);
            let mut group = group.clone();
            group.title = name.into();
            std::thread::spawn(move || {
                store.import_prepared(group, |dir, _| {
                    fs::write(dir.join("ledger.jsonl"), name)?;
                    barrier.wait();
                    Ok(())
                })
            })
        });
        let results = handles.map(|handle| handle.join().expect("join"));
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let loaded = store.load("g_collision").expect("winner");
        assert_eq!(
            fs::read_to_string(store.ledger_path("g_collision").expect("path")).expect("ledger"),
            loaded.title
        );
        assert_eq!(
            store
                .list()
                .expect("registry")
                .iter()
                .filter(|meta| meta.group_id == "g_collision")
                .count(),
            1
        );
    }

    #[test]
    fn import_does_not_reuse_a_registered_id_whose_directory_is_temporarily_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("source", "").expect("source");
        let dir = store.group_dir(&group.group_id).expect("path");
        let tombstone = home.groups_dir().join(".test-deleting");
        fs::rename(&dir, &tombstone).expect("move directory before registry removal");

        let error = store
            .import(group.clone())
            .expect_err("registered ID is occupied");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!dir.exists());
        assert!(tombstone.is_dir());
        assert!(
            store
                .list()
                .expect("registry")
                .iter()
                .any(|meta| meta.group_id == group.group_id)
        );
    }

    #[test]
    fn a_registry_conflict_after_preparation_never_publishes_the_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("source", "").expect("source");
        let mut occupied = store
            .list()
            .expect("registry")
            .into_iter()
            .find(|meta| meta.group_id == group.group_id)
            .expect("source metadata");
        group.group_id = "g_late_conflict".into();
        occupied.group_id.clone_from(&group.group_id);
        occupied.title = "other registration".into();
        let id = group.group_id.clone();
        let error = store
            .import_prepared(group, |_, _| {
                // The preflight already ran. Another writer claims the ID
                // before the importing writer acquires the final registry lock.
                Registry::mutate(&home, |registry| {
                    registry.groups.insert(occupied.group_id.clone(), occupied);
                    Ok(())
                })
            })
            .expect_err("registration conflict");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let retained = store.group_dir(&id).expect("path");
        assert!(retained.is_dir());
        assert!(!retained.join("group.yaml").exists());
        assert!(error.to_string().contains(&retained.display().to_string()));
        assert_eq!(
            store
                .list()
                .expect("registry")
                .into_iter()
                .find(|meta| meta.group_id == id)
                .expect("other entry")
                .title,
            "other registration"
        );
    }

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

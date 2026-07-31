use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

use crate::{GroupStore, Registry};

pub(crate) fn delete(store: &GroupStore, group_id: &str) -> io::Result<bool> {
    delete_with(store, group_id, |path| fs::remove_dir_all(path))
}

fn delete_with(
    store: &GroupStore,
    group_id: &str,
    remove_tombstone: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    delete_with_steps(store, group_id, remove_tombstone, unregister)
}

fn delete_with_steps(
    store: &GroupStore,
    group_id: &str,
    remove_tombstone: impl FnOnce(&Path) -> io::Result<()>,
    unregister_group: impl Fn(&GroupStore, &str) -> io::Result<()>,
) -> io::Result<bool> {
    let dir = store.group_dir(group_id)?;
    if !dir.exists() {
        unregister_group(store, group_id)?;
        cleanup_tombstones(store, group_id)?;
        return Ok(false);
    }
    let ledger_path = dir.join("ledger.jsonl");
    let tombstone = store
        .home()
        .groups_dir()
        .join(format!(".{group_id}.deleting-{}", Uuid::new_v4().simple()));
    fs::rename(&dir, &tombstone)?;
    unregister_group(store, group_id).map_err(|error| match fs::rename(&tombstone, &dir) {
        Ok(()) => error,
        Err(rollback) => io::Error::other(format!(
            "{error}; rollback_failed: could not restore {}: {rollback}",
            dir.display()
        )),
    })?;
    crate::ledger_index::invalidate_path(&ledger_path);
    remove_tombstone(&tombstone).map_err(|error| {
        io::Error::other(format!(
            "rollback_failed: group was unregistered but tombstone {} could not be removed: {error}",
            tombstone.display()
        ))
    })?;
    Ok(true)
}

fn unregister(store: &GroupStore, group_id: &str) -> io::Result<()> {
    Registry::mutate(store.home(), |registry| {
        registry.groups.remove(group_id);
        registry.defaults.retain(|_, value| value != group_id);
        Ok(())
    })
}

fn cleanup_tombstones(store: &GroupStore, group_id: &str) -> io::Result<()> {
    let prefix = format!(".{group_id}.deleting-");
    for entry in fs::read_dir(store.home().groups_dir())? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HomeLayout;

    #[test]
    fn failed_tombstone_cleanup_is_explicit_and_retryable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("cleanup", "").expect("group");

        let error = delete_with(&store, &group.group_id, |_| {
            Err(io::Error::other("injected cleanup failure"))
        })
        .expect_err("cleanup must fail explicitly");
        assert!(error.to_string().contains("rollback_failed"));
        assert!(!store.group_dir(&group.group_id).expect("dir").exists());
        assert!(
            !Registry::load(&home)
                .expect("registry")
                .groups
                .contains_key(&group.group_id)
        );
        assert!(has_tombstone(&home, &group.group_id));

        assert!(!delete(&store, &group.group_id).expect("retry cleanup"));
        assert!(!has_tombstone(&home, &group.group_id));
    }

    #[test]
    fn deleting_a_group_invalidates_its_original_ledger_index_key() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("cached", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        crate::ledger::append(
            &ledger_path,
            &cccc_contracts::Event::new("chat.message", &group.group_id),
        )
        .expect("append event");
        crate::ledger::read_all(&ledger_path).expect("populate index");
        assert!(crate::ledger_index::is_cached(&ledger_path));

        assert!(delete(&store, &group.group_id).expect("delete group"));

        assert!(
            !crate::ledger_index::is_cached(&ledger_path),
            "the cache key is the pre-rename ledger path"
        );
    }

    #[test]
    fn retry_keeps_the_only_tombstone_when_unregister_still_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("recoverable", "").expect("group");
        let directory = store.group_dir(&group.group_id).expect("group directory");
        let tombstone = home
            .groups_dir()
            .join(format!(".{}.deleting-recovery", group.group_id));
        std::fs::rename(&directory, &tombstone).expect("simulate failed rollback");

        let error = delete_with_steps(
            &store,
            &group.group_id,
            |path| std::fs::remove_dir_all(path),
            |_, _| Err(io::Error::other("injected registry failure")),
        )
        .expect_err("unregister failure");

        assert!(error.to_string().contains("injected registry failure"));
        assert!(tombstone.is_dir(), "the only data copy must remain");
        assert!(
            Registry::load(&home)
                .expect("registry")
                .groups
                .contains_key(&group.group_id)
        );
    }

    fn has_tombstone(home: &HomeLayout, group_id: &str) -> bool {
        home.groups_dir().read_dir().expect("entries").any(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(&format!(".{group_id}.deleting-"))
        })
    }
}

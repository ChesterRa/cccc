use cccc_core::{GroupStore, HomeLayout, active};

use crate::dispatch::OpError;

pub(super) fn run(
    home: &HomeLayout,
    store: &GroupStore,
    group_id: &str,
    restore_active: Option<Option<String>>,
    original: OpError,
) -> OpError {
    let mut failures = Vec::new();
    if let Some(previous) = restore_active {
        let restored = match previous {
            Some(group_id) => active::set(home, &group_id),
            None => active::clear(home),
        };
        if let Err(error) = restored {
            failures.push(format!("active: {error}"));
        }
    }
    if let Err(error) = store.delete(group_id) {
        failures.push(format!("group: {error}"));
    }
    if failures.is_empty() {
        original
    } else {
        OpError::new(
            "rollback_failed",
            format!(
                "{}; rollback failed: {}",
                original.message,
                failures.join("; ")
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_previous_active_and_removes_created_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let previous = store.create("previous", "").expect("previous");
        let created = store.create("created", "").expect("created");
        active::set(&home, &created.group_id).expect("created active");

        let error = run(
            &home,
            &store,
            &created.group_id,
            Some(Some(previous.group_id.clone())),
            OpError::new("active_failed", "active failed"),
        );

        assert_eq!(error.code, "active_failed");
        assert_eq!(active::get(&home).expect("active"), Some(previous.group_id));
        assert!(!store.group_dir(&created.group_id).expect("path").exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_failure_is_reported_as_rollback_failed() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create("created", "").expect("created");
        let groups = home.groups_dir();
        std::fs::set_permissions(&groups, std::fs::Permissions::from_mode(0o555))
            .expect("lock groups");
        let error = run(
            &home,
            &store,
            &created.group_id,
            None,
            OpError::new("ledger_failed", "ledger failed"),
        );
        std::fs::set_permissions(&groups, std::fs::Permissions::from_mode(0o755))
            .expect("unlock groups");

        assert_eq!(error.code, "rollback_failed");
        assert!(store.group_dir(&created.group_id).expect("path").exists());
        store.delete(&created.group_id).expect("cleanup");
    }
}

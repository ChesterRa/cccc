use cccc_contracts::Actor;
use cccc_core::{GroupStore, HomeLayout};
use std::collections::BTreeMap;

use crate::dispatch::OpError;

pub(super) fn rollback_added(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    original: OpError,
) -> OpError {
    let store = match GroupStore::new(home.clone()).map_err(OpError::io) {
        Ok(store) => store,
        Err(error) => return rollback_error(original, error),
    };
    if let Err(error) = store
        .mutate(group_id, |group| {
            group.actors.retain(|actor| actor.id != actor_id);
            Ok(())
        })
        .map_err(OpError::io)
    {
        return rollback_error(original, error);
    }
    if let Err(error) = super::actor_secrets::remove(home, group_id, actor_id) {
        return rollback_error(original, error);
    }
    original
}

pub(super) fn restore_removed(
    home: &HomeLayout,
    group_id: &str,
    actor: Actor,
    index: usize,
    secrets: BTreeMap<String, String>,
    original: OpError,
) -> OpError {
    let actor_id = actor.id.clone();
    let store = match GroupStore::new(home.clone()).map_err(OpError::io) {
        Ok(store) => store,
        Err(error) => return rollback_error(original, error),
    };
    if let Err(error) = store
        .mutate(group_id, |group| {
            if !group.actors.iter().any(|item| item.id == actor_id) {
                group.actors.insert(index.min(group.actors.len()), actor);
            }
            Ok(())
        })
        .map_err(OpError::io)
    {
        return rollback_error(original, error);
    }
    if let Err(error) = super::actor_secrets::replace(home, group_id, &actor_id, secrets) {
        return rollback_error(original, error);
    }
    original
}

fn rollback_error(original: OpError, rollback: OpError) -> OpError {
    OpError::new(
        "rollback_failed",
        format!(
            "{}; rollback failed: {}",
            original.message, rollback.message
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use cccc_core::actors;

    fn fixture() -> (tempfile::TempDir, HomeLayout, GroupStore, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("actor saga", "").expect("group");
        (temp, home, store, group.group_id)
    }

    #[test]
    fn added_actor_and_secrets_are_removed_after_later_failure() {
        let (_temp, home, store, group_id) = fixture();
        store
            .mutate(&group_id, |group| actors::add(group, Actor::new("peer")))
            .expect("add");
        super::super::actor_secrets::replace(
            &home,
            &group_id,
            "peer",
            BTreeMap::from([("TOKEN".into(), "secret".into())]),
        )
        .expect("secrets");

        let error = rollback_added(
            &home,
            &group_id,
            "peer",
            OpError::new("ledger_failed", "injected ledger failure"),
        );

        assert_eq!(error.code, "ledger_failed");
        assert!(store.load(&group_id).expect("group").actors.is_empty());
        assert!(
            super::super::actor_secrets::values(&home, &group_id, "peer")
                .expect("secrets")
                .is_empty()
        );
    }

    #[test]
    fn removed_actor_and_secrets_are_restored_after_later_failure() {
        let (_temp, home, store, group_id) = fixture();
        let actor = Actor::new("peer");
        let secrets = BTreeMap::from([("TOKEN".into(), "secret".into())]);

        let error = restore_removed(
            &home,
            &group_id,
            actor,
            0,
            secrets.clone(),
            OpError::new("ledger_failed", "injected ledger failure"),
        );

        assert_eq!(error.code, "ledger_failed");
        assert_eq!(store.load(&group_id).expect("group").actors[0].id, "peer");
        assert_eq!(
            super::super::actor_secrets::values(&home, &group_id, "peer").expect("secrets"),
            secrets
        );
    }

    #[test]
    fn failed_compensation_is_reported() {
        let (_temp, home, store, group_id) = fixture();
        std::fs::remove_file(
            store
                .group_dir(&group_id)
                .expect("group dir")
                .join("group.yaml"),
        )
        .expect("remove group document");

        let error = rollback_added(
            &home,
            &group_id,
            "peer",
            OpError::new("ledger_failed", "injected ledger failure"),
        );

        assert_eq!(error.code, "rollback_failed");
        assert!(error.message.contains("injected ledger failure"));
    }
}

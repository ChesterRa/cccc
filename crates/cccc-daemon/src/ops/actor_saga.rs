use cccc_contracts::Actor;
use cccc_core::{GroupStore, HomeLayout, web_model_connectors};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::dispatch::OpError;

pub(super) struct RemovedActorSnapshot {
    pub(super) actor: Actor,
    pub(super) index: usize,
    pub(super) web_model_target: Option<Value>,
    pub(super) web_model_delivery_preference: Option<Value>,
    pub(super) runtime_state: Option<Value>,
    pub(super) connector_entries: Vec<Value>,
    pub(super) secrets: BTreeMap<String, String>,
}

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
    snapshot: RemovedActorSnapshot,
    original: OpError,
) -> OpError {
    let RemovedActorSnapshot {
        actor,
        index,
        web_model_target,
        web_model_delivery_preference,
        runtime_state,
        connector_entries,
        secrets,
    } = snapshot;
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
            if let Some(target) = web_model_target {
                let targets = group
                    .extra
                    .entry("web_model_browser_targets")
                    .or_insert_with(|| Value::Object(Default::default()))
                    .as_object_mut()
                    .ok_or_else(|| std::io::Error::other("invalid web model target store"))?;
                targets.insert(actor_id.clone(), target);
            }
            if let Some(preference) = web_model_delivery_preference {
                let preferences = group
                    .extra
                    .entry("web_model_delivery_preferences")
                    .or_insert_with(|| Value::Object(Default::default()))
                    .as_object_mut()
                    .ok_or_else(|| std::io::Error::other("invalid web model preference store"))?;
                preferences.insert(actor_id.clone(), preference);
            }
            if let Some(state) = runtime_state {
                let states = group
                    .extra
                    .entry("runtime_states")
                    .or_insert_with(|| Value::Object(Default::default()))
                    .as_object_mut()
                    .ok_or_else(|| std::io::Error::other("invalid runtime state store"))?;
                states.insert(actor_id.clone(), state);
            }
            Ok(())
        })
        .map_err(OpError::io)
    {
        return rollback_error(original, error);
    }
    if let Err(error) = web_model_connectors::restore(home, &connector_entries).map_err(OpError::io)
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
    use std::collections::BTreeMap;

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
    fn removed_actor_state_and_retired_secrets_are_restored_after_failure() {
        let (_temp, home, store, group_id) = fixture();
        let actor = Actor::new("peer");
        let secrets = BTreeMap::from([("TOKEN".into(), "secret".into())]);
        super::super::actor_secrets::replace(&home, &group_id, "peer", secrets.clone())
            .expect("secrets");
        let target = serde_json::json!({
            "state":"bound_existing_chat",
            "kind":"existing_chat",
            "url":"https://chatgpt.com/c/restore-me"
        });
        let preference = serde_json::json!({
            "mode":"image_compat",
            "updated_at":"2026-08-11T00:00:00Z",
            "updated_by":"user"
        });
        let runtime_state = serde_json::json!({
            "status":"working",
            "task_id":"restore-task"
        });
        let connector = serde_json::json!({
            "connector_id":"wmc_restore",
            "group_id":group_id,
            "actor_id":"peer",
            "secret":"wmcs_restore",
            "created_at":"2026-08-11T00:00:00Z",
            "updated_at":"2026-08-11T00:00:00Z",
            "revoked":false
        });
        web_model_connectors::replace_active(&home, &connector).expect("connector");
        let retired = web_model_connectors::retire_actor(&home, &group_id, "peer")
            .expect("retired connector");
        super::super::actor_secrets::remove(&home, &group_id, "peer").expect("retire secrets");

        let error = restore_removed(
            &home,
            &group_id,
            RemovedActorSnapshot {
                actor,
                index: 0,
                web_model_target: Some(target.clone()),
                web_model_delivery_preference: Some(preference.clone()),
                runtime_state: Some(runtime_state.clone()),
                connector_entries: retired,
                secrets: secrets.clone(),
            },
            OpError::new("ledger_failed", "injected ledger failure"),
        );

        assert_eq!(error.code, "ledger_failed");
        assert_eq!(store.load(&group_id).expect("group").actors[0].id, "peer");
        assert_eq!(
            super::super::actor_secrets::values(&home, &group_id, "peer").expect("secrets"),
            secrets,
            "the pre-commit removal path must restore retired secrets"
        );
        assert_eq!(
            store.load(&group_id).expect("group").extra["web_model_browser_targets"]["peer"],
            target
        );
        assert_eq!(
            store.load(&group_id).expect("group").extra["web_model_delivery_preferences"]["peer"],
            preference
        );
        assert_eq!(
            store.load(&group_id).expect("group").extra["runtime_states"]["peer"],
            runtime_state
        );
        let connectors = web_model_connectors::load(&home).expect("connectors");
        assert!(
            !connectors
                .iter()
                .find(|item| item["connector_id"] == "wmc_restore")
                .expect("restored connector")["revoked"]
                .as_bool()
                .unwrap_or(true)
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

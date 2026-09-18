use cccc_contracts::{DaemonRequest, GroupState};
use cccc_core::group::AUTOMATION_TIMING_KEYS;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Registry, active, permissions};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub(super) fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    if required_arg(request, "confirm")? != group_id {
        return Err(OpError::new(
            "confirm_required",
            format!("confirm must equal group_id: {group_id}"),
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let old = store.load(&group_id).map_err(OpError::not_found)?;
    permissions::require_group(
        &old,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(OpError::invalid)?;
    let was_active = active::get(home).map_err(OpError::io)?.as_deref() == Some(&group_id);
    let created = store.create(&old.title, &old.topic).map_err(OpError::io)?;
    let replacement = prepare_replacement(&store, &old, &created)
        .and_then(|replacement| {
            super::actor_secrets::copy_group(home, &old.group_id, &replacement.group_id)?;
            super::groups::append_group_event(
                home,
                &replacement,
                "group.create",
                request,
                json!({
                    "title":replacement.title,
                    "topic":replacement.topic,
                    "reset_from":old.group_id
                }),
            )?;
            Ok(replacement)
        })
        .map_err(|error| rollback_new(&store, &old, &created.group_id, error))?;

    super::actor_delivery::shutdown_group(&old.group_id);
    if let Err(error) = super::actor_runtime::stop_group(&old) {
        return Err(rollback_new(&store, &old, &replacement.group_id, error));
    }
    if was_active && let Err(error) = active::set(home, &replacement.group_id).map_err(OpError::io)
    {
        return Err(rollback_new(&store, &old, &replacement.group_id, error));
    }
    let old_delete_error = settle_old_delete(&store, &old, store.delete(&old.group_id));
    if old_delete_error.is_none() {
        super::actor_secrets::remove_group(home, &old.group_id)?;
    }
    object(json!({
        "old_group_id":old.group_id,
        "new_group_id":replacement.group_id,
        "group_id":replacement.group_id,
        "deleted_old":old_delete_error.is_none(),
        "old_delete_error":old_delete_error,
        "active_group_id":was_active.then_some(&replacement.group_id),
        "group":super::group_runtime::group(replacement),
    }))
}

/// Reports a failed delete of the old Group. The replacement already carries
/// the old actors, so an old Group that survives must give up its ChatGPT Web
/// Model actor: the instance allows only one.
fn settle_old_delete(
    store: &GroupStore,
    old: &GroupDoc,
    deleted: std::io::Result<bool>,
) -> Option<String> {
    let error = deleted.err()?;
    // A Group whose directory is already gone must not be resurrected by mutate.
    if store.load(&old.group_id).is_err() {
        return Some(error.to_string());
    }
    let released = (|| -> std::io::Result<bool> {
        let profiles = cccc_core::profiles::ProfileStore::new(store.home().clone())?;
        let mut retired = Vec::new();
        for actor in &old.actors {
            if cccc_core::actors::reserves_web_model(&profiles, actor)? {
                retired.push(actor.id.clone());
            }
        }
        if retired.is_empty() {
            return Ok(false);
        }
        store.mutate(&old.group_id, |document| {
            document.actors.retain(|actor| !retired.contains(&actor.id));
            Ok(())
        })?;
        Ok(true)
    })();
    Some(match released {
        Ok(true) => format!(
            "{error}; removed its ChatGPT Web Model actor because the replacement now holds it"
        ),
        Ok(false) => error.to_string(),
        Err(release) => format!("{error}; could not remove its ChatGPT Web Model actor: {release}"),
    })
}

fn prepare_replacement(
    store: &GroupStore,
    old: &GroupDoc,
    created: &GroupDoc,
) -> Result<GroupDoc, OpError> {
    let new_group_id = created.group_id.clone();
    let replacement = store
        .mutate(&new_group_id, |document| {
            *document = created.clone();
            document.title.clone_from(&old.title);
            document.topic.clone_from(&old.topic);
            document.running = false;
            document.state = GroupState::Active;
            document.active_scope_key.clone_from(&old.active_scope_key);
            document.scopes.clone_from(&old.scopes);
            document.actors.clone_from(&old.actors);
            document.automation = reset_automation(old);
            for actor in &mut document.actors {
                actor.avatar_asset_path.clear();
            }
            Ok(document.clone())
        })
        .map_err(OpError::io)?;
    Registry::mutate(store.home(), |registry| {
        if let Some(meta) = registry.groups.get_mut(&new_group_id) {
            meta.default_scope_key
                .clone_from(&replacement.active_scope_key);
        }
        for scope in &replacement.scopes {
            registry
                .defaults
                .insert(scope.scope_key.clone(), new_group_id.clone());
        }
        Ok(())
    })
    .map_err(OpError::io)?;
    Ok(replacement)
}

fn reset_automation(group: &GroupDoc) -> Map<String, Value> {
    let mut automation = Map::new();
    for key in ["version", "rules", "snippets", "snippet_overrides"] {
        if let Some(value) = group.automation.get(key) {
            automation.insert(key.into(), value.clone());
        }
    }
    for key in AUTOMATION_TIMING_KEYS {
        if let Some(value) = group.automation.get(*key).or_else(|| {
            group
                .extra
                .get("settings")
                .and_then(Value::as_object)
                .and_then(|settings| settings.get(*key))
        }) {
            automation.insert((*key).into(), value.clone());
        }
    }
    automation
}

fn rollback_new(store: &GroupStore, old: &GroupDoc, group_id: &str, original: OpError) -> OpError {
    let mut failures = Vec::new();
    if let Err(error) = super::actor_secrets::remove_group(store.home(), group_id) {
        failures.push(format!("private_env: {}", error.message));
    }
    if let Err(error) = store.delete(group_id) {
        failures.push(format!("group: {error}"));
    }
    if let Err(error) = Registry::mutate(store.home(), |registry| {
        for scope in &old.scopes {
            registry
                .defaults
                .insert(scope.scope_key.clone(), old.group_id.clone());
        }
        Ok(())
    }) {
        failures.push(format!("scope_registry: {error}"));
    }
    if failures.is_empty() {
        original
    } else {
        OpError::new(
            "rollback_failed",
            format!(
                "{}; failed to roll back replacement {group_id}: {}",
                original.message,
                failures.join("; ")
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::{group_scope, scope};

    #[test]
    fn replacement_keeps_only_declared_configuration_and_normalizes_automation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let old = store
            .create("reset source", "keep topic")
            .expect("old group");
        let old = store
            .mutate(&old.group_id, |group| {
                group.automation = json!({
                    "version":7,
                    "rules":[{"id":"keep-rule"}],
                    "nudge_after_seconds":123,
                    "runtime_last_tick":"drop"
                })
                .as_object()
                .cloned()
                .expect("automation object");
                group.extra.insert(
                    "settings".into(),
                    json!({
                        "nudge_after_seconds":999,
                        "help_nudge_interval_seconds":777,
                        "default_send_to":"broadcast"
                    }),
                );
                for key in [
                    "runtime_states",
                    "assistants",
                    "im",
                    "im_bridge",
                    "group_bridge",
                    "web_model_delivery_preferences",
                ] {
                    group.extra.insert(key.into(), json!({"drop":true}));
                }
                Ok(group.clone())
            })
            .expect("seed source");
        let created = store.create("replacement", "").expect("replacement");

        let replacement = prepare_replacement(&store, &old, &created).expect("prepare");

        assert_eq!(replacement.title, "reset source");
        assert_eq!(replacement.topic, "keep topic");
        assert_eq!(replacement.automation["version"], json!(7));
        assert_eq!(replacement.automation["rules"], json!([{"id":"keep-rule"}]));
        assert!(!replacement.automation.contains_key("nudge_after_seconds"));
        assert_eq!(
            replacement.automation["help_nudge_interval_seconds"],
            json!(777)
        );
        assert!(!replacement.automation.contains_key("runtime_last_tick"));
        assert!(replacement.extra.is_empty());
    }

    #[test]
    fn rollback_removes_replacement_group_secrets_and_scope_routing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let old = store.create("reset source", "").expect("old group");
        let project = temp.path().join("project");
        std::fs::create_dir(&project).expect("project");
        let detected = scope::detect(&project).expect("scope");
        let old = group_scope::attach(&store, &old.group_id, detected.clone()).expect("attach");
        let created = store.create("replacement", "").expect("replacement");
        let replacement = prepare_replacement(&store, &old, &created).expect("prepare");
        let secret_dir = home
            .root()
            .join("state/secrets/actors")
            .join(&replacement.group_id);
        std::fs::create_dir_all(&secret_dir).expect("replacement secrets");
        std::fs::write(secret_dir.join("partial.json"), b"{}\n").expect("partial secret");

        let error = rollback_new(
            &store,
            &old,
            &replacement.group_id,
            OpError::new("injected", "copy failed"),
        );

        assert_eq!(error.code, "injected");
        assert!(store.load(&old.group_id).is_ok());
        assert!(store.load(&replacement.group_id).is_err());
        assert!(!secret_dir.exists());
        let registry = Registry::load(&home).expect("registry");
        assert!(!registry.groups.contains_key(&replacement.group_id));
        assert_eq!(
            registry.defaults.get(&detected.scope_key),
            Some(&old.group_id)
        );
    }
}

#[cfg(test)]
mod settle_old_delete_tests {
    use super::*;
    use cccc_contracts::Actor;

    fn home_with_web_model_group() -> (tempfile::TempDir, GroupStore, GroupDoc) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let created = store.create("old", "").expect("group");
        let old = store
            .mutate(&created.group_id, |document| {
                let mut actor = Actor::new("web");
                actor.runtime = cccc_contracts::ActorRuntime::WebModel;
                document.actors.push(actor);
                document.actors.push(Actor::new("peer"));
                Ok(document.clone())
            })
            .expect("actors");
        (temp, store, old)
    }

    #[test]
    fn a_successful_delete_reports_nothing() {
        let (_temp, store, old) = home_with_web_model_group();
        assert_eq!(settle_old_delete(&store, &old, Ok(true)), None);
    }

    #[test]
    fn an_undeleted_old_group_releases_its_web_model_actor() {
        let (_temp, store, old) = home_with_web_model_group();
        let message = settle_old_delete(&store, &old, Err(std::io::Error::other("injected")))
            .expect("delete failure is reported");
        assert!(message.contains("injected"), "{message}");
        assert!(
            message.contains("removed its ChatGPT Web Model actor"),
            "{message}"
        );
        let ids = store
            .load(&old.group_id)
            .expect("old group survives")
            .actors
            .iter()
            .map(|actor| actor.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["peer".to_owned()]);
    }

    #[test]
    fn an_undeleted_group_releases_a_pending_profile_owner() {
        let (_temp, store, old) = home_with_web_model_group();
        let profiles =
            cccc_core::profiles::ProfileStore::new(store.home().clone()).expect("profiles");
        profiles
            .upsert(
                json!({"id":"pending","runtime":"web_model"})
                    .as_object()
                    .expect("profile object")
                    .clone(),
                None,
            )
            .expect("profile");
        let old = store
            .mutate(&old.group_id, |document| {
                document.actors[0].runtime = cccc_contracts::ActorRuntime::Codex;
                document.actors[0].profile_id = "pending".into();
                Ok(document.clone())
            })
            .expect("pending actor");
        let message = settle_old_delete(&store, &old, Err(std::io::Error::other("injected")))
            .expect("reported");
        assert!(
            message.contains("removed its ChatGPT Web Model actor"),
            "{message}"
        );
        assert_eq!(store.load(&old.group_id).expect("old").actors.len(), 1);
        assert_eq!(store.load(&old.group_id).expect("old").actors[0].id, "peer");
    }

    #[test]
    fn a_vanished_old_group_is_not_resurrected() {
        let (_temp, store, old) = home_with_web_model_group();
        let directory = store.group_dir(&old.group_id).expect("group dir");
        std::fs::remove_dir_all(&directory).expect("remove old group");
        let message = settle_old_delete(&store, &old, Err(std::io::Error::other("injected")))
            .expect("delete failure is reported");
        assert_eq!(message, "injected");
        assert!(
            !directory.exists(),
            "mutate must not recreate the group directory"
        );
    }
}

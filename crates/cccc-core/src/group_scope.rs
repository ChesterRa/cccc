use cccc_contracts::utc_now;
use std::io;

use crate::{GroupDoc, GroupStore, Registry, Scope};

#[must_use]
pub fn resolve_attached_scope<'a>(group: &'a GroupDoc, reference: &str) -> Option<&'a Scope> {
    let wanted = reference.trim();
    if wanted.is_empty() {
        return None;
    }
    group.scopes.iter().find(|scope| {
        scope.scope_key == wanted
            || scope.url == wanted
            || paths_resolve_to_same_location(&scope.url, wanted)
    })
}

pub fn normalize_actor_scope_keys(group: &mut GroupDoc) -> usize {
    let active_scope_key =
        resolve_attached_scope(group, &group.active_scope_key).map(|scope| scope.scope_key.clone());
    let replacements = group
        .actors
        .iter()
        .enumerate()
        .filter_map(|(index, actor)| {
            let reference = actor.default_scope_key.trim();
            if reference.is_empty() {
                return None;
            }
            let scope_key = resolve_attached_scope(group, reference)
                .map(|scope| scope.scope_key.clone())
                .or_else(|| active_scope_key.clone())?;
            (scope_key != reference).then_some((index, scope_key))
        })
        .collect::<Vec<_>>();
    for (index, scope_key) in &replacements {
        group.actors[*index].default_scope_key.clone_from(scope_key);
    }
    replacements.len()
}

fn paths_resolve_to_same_location(left: &str, right: &str) -> bool {
    let Ok(left) = std::path::Path::new(left).canonicalize() else {
        return false;
    };
    let Ok(right) = std::path::Path::new(right).canonicalize() else {
        return false;
    };
    left == right
}

pub fn attach(store: &GroupStore, group_id: &str, scope: Scope) -> io::Result<GroupDoc> {
    attach_with(store, group_id, scope, |result| {
        Registry::mutate(store.home(), |registry| {
            registry
                .defaults
                .insert(result.active_scope_key.clone(), group_id.into());
            if let Some(meta) = registry.groups.get_mut(group_id) {
                meta.default_scope_key.clone_from(&result.active_scope_key);
                meta.updated_at = utc_now();
            }
            Ok(())
        })
    })
}

fn attach_with(
    store: &GroupStore,
    group_id: &str,
    scope: Scope,
    update_registry: impl FnOnce(&GroupDoc) -> io::Result<()>,
) -> io::Result<GroupDoc> {
    store.mutate_with_rollback(
        group_id,
        |group| {
            if let Some(existing) = group
                .scopes
                .iter_mut()
                .find(|item| item.scope_key == scope.scope_key)
            {
                existing.clone_from(&scope);
            } else {
                group.scopes.push(scope.clone());
            }
            group.active_scope_key.clone_from(&scope.scope_key);
            Ok(group.clone())
        },
        update_registry,
    )
}

pub fn detach(store: &GroupStore, group_id: &str, scope_key: &str) -> io::Result<GroupDoc> {
    detach_with(store, group_id, scope_key, |result| {
        Registry::mutate(store.home(), |registry| {
            if registry.defaults.get(scope_key).map(String::as_str) == Some(group_id) {
                registry.defaults.remove(scope_key);
            }
            if let Some(meta) = registry.groups.get_mut(group_id) {
                meta.default_scope_key.clone_from(&result.active_scope_key);
            }
            Ok(())
        })
    })
}

fn detach_with(
    store: &GroupStore,
    group_id: &str,
    scope_key: &str,
    update_registry: impl FnOnce(&GroupDoc) -> io::Result<()>,
) -> io::Result<GroupDoc> {
    store.mutate_with_rollback(
        group_id,
        |group| {
            let before = group.scopes.len();
            group.scopes.retain(|scope| scope.scope_key != scope_key);
            if before == group.scopes.len() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "scope not attached",
                ));
            }
            if group.active_scope_key == scope_key {
                group.active_scope_key = group
                    .scopes
                    .first()
                    .map(|scope| scope.scope_key.clone())
                    .unwrap_or_default();
            }
            for actor in &mut group.actors {
                if actor.default_scope_key == scope_key {
                    actor.default_scope_key.clone_from(&group.active_scope_key);
                }
            }
            Ok(group.clone())
        },
        update_registry,
    )
}

pub fn activate(store: &GroupStore, group_id: &str, scope_key: &str) -> io::Result<GroupDoc> {
    activate_with(store, group_id, scope_key, |result| {
        Registry::mutate(store.home(), |registry| {
            registry.defaults.insert(scope_key.into(), group_id.into());
            if let Some(meta) = registry.groups.get_mut(group_id) {
                meta.default_scope_key.clone_from(&result.active_scope_key);
            }
            Ok(())
        })
    })
}

fn activate_with(
    store: &GroupStore,
    group_id: &str,
    scope_key: &str,
    update_registry: impl FnOnce(&GroupDoc) -> io::Result<()>,
) -> io::Result<GroupDoc> {
    store.mutate_with_rollback(
        group_id,
        |group| {
            if !group
                .scopes
                .iter()
                .any(|scope| scope.scope_key == scope_key)
            {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "scope not attached",
                ));
            }
            group.active_scope_key = scope_key.into();
            Ok(group.clone())
        },
        update_registry,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HomeLayout;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn legacy_actor_scope_paths_normalize_to_attached_scope_keys() {
        let temp = tempfile::tempdir().expect("tempdir");
        let scope_path = temp.path().join("project");
        std::fs::create_dir(&scope_path).expect("project");
        let mut group =
            GroupStore::new(HomeLayout::from_path(temp.path().join("home")).expect("home"))
                .expect("store")
                .create("scope migration", "")
                .expect("group");
        group.scopes.push(Scope {
            scope_key: "s_project".into(),
            url: scope_path.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        });
        let mut actor = cccc_contracts::Actor::new("peer");
        actor.default_scope_key = scope_path.to_string_lossy().into_owned();
        group.actors.push(actor);

        assert_eq!(normalize_actor_scope_keys(&mut group), 1);
        assert_eq!(group.actors[0].default_scope_key, "s_project");
        assert_eq!(normalize_actor_scope_keys(&mut group), 0);
    }

    #[test]
    fn stale_actor_scope_keys_fall_back_to_the_active_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let scope_path = temp.path().join("project");
        std::fs::create_dir(&scope_path).expect("project");
        let mut group =
            GroupStore::new(HomeLayout::from_path(temp.path().join("home")).expect("home"))
                .expect("store")
                .create("scope repair", "")
                .expect("group");
        group.scopes.push(Scope {
            scope_key: "s_active".into(),
            url: scope_path.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_active".into();
        let mut actor = cccc_contracts::Actor::new("peer");
        actor.default_scope_key = "s_detached".into();
        group.actors.push(actor);

        assert_eq!(normalize_actor_scope_keys(&mut group), 1);
        assert_eq!(group.actors[0].default_scope_key, "s_active");
    }

    #[test]
    fn detach_reassigns_actor_scope_to_the_remaining_active_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("scope detach", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                group.scopes = vec![
                    Scope {
                        scope_key: "s_old".into(),
                        url: temp.path().join("old").to_string_lossy().into_owned(),
                        label: "old".into(),
                        git_remote: String::new(),
                    },
                    Scope {
                        scope_key: "s_next".into(),
                        url: temp.path().join("next").to_string_lossy().into_owned(),
                        label: "next".into(),
                        git_remote: String::new(),
                    },
                ];
                group.active_scope_key = "s_old".into();
                let mut actor = cccc_contracts::Actor::new("peer");
                actor.default_scope_key = "s_old".into();
                group.actors.push(actor);
                Ok(())
            })
            .expect("configure group");

        let detached = detach_with(&store, &group.group_id, "s_old", |_| Ok(())).expect("detach");

        assert_eq!(detached.active_scope_key, "s_next");
        assert_eq!(detached.actors[0].default_scope_key, "s_next");
    }

    #[test]
    fn detaching_shared_scope_from_non_default_group_preserves_current_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let first = store.create("first", "").expect("first group");
        let second = store.create("second", "").expect("second group");
        let scope = Scope {
            scope_key: "s_shared".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "shared".into(),
            git_remote: String::new(),
        };
        attach(&store, &first.group_id, scope.clone()).expect("attach first");
        attach(&store, &second.group_id, scope).expect("attach second");

        let detached = detach(&store, &first.group_id, "s_shared").expect("detach first");

        assert!(detached.scopes.is_empty());
        assert_eq!(
            Registry::load(store.home())
                .expect("registry")
                .defaults
                .get("s_shared"),
            Some(&second.group_id)
        );
    }

    #[test]
    fn failed_attach_rollback_does_not_overwrite_a_concurrent_group_update() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("before", "").expect("group");
        let group_id = group.group_id.clone();
        let scope = Scope {
            scope_key: "s_test".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "test".into(),
            git_remote: String::new(),
        };
        let (side_effect_started_tx, side_effect_started_rx) = mpsc::channel();
        let (fail_side_effect_tx, fail_side_effect_rx) = mpsc::channel();
        let (update_started_tx, update_started_rx) = mpsc::channel();
        let (update_finished_tx, update_finished_rx) = mpsc::channel();

        std::thread::scope(|threads| {
            let attach_store = store.clone();
            let attach_group_id = group_id.clone();
            let attach = threads.spawn(move || {
                attach_with(&attach_store, &attach_group_id, scope, |_| {
                    side_effect_started_tx.send(()).expect("signal");
                    fail_side_effect_rx.recv().expect("continue");
                    Err(io::Error::other("injected registry failure"))
                })
            });
            side_effect_started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("side effect started");

            let update_store = store.clone();
            let update_group_id = group_id.clone();
            let update = threads.spawn(move || {
                update_started_tx.send(()).expect("update started");
                let result = update_store.update(&update_group_id, Some("concurrent"), None);
                update_finished_tx.send(()).expect("update finished");
                result
            });
            update_started_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("update started");
            assert!(
                update_finished_rx
                    .recv_timeout(Duration::from_millis(100))
                    .is_err(),
                "concurrent update must wait for the scope transaction"
            );
            fail_side_effect_tx.send(()).expect("fail side effect");

            let attach_error = attach.join().expect("attach thread").expect_err("failure");
            assert!(
                attach_error
                    .to_string()
                    .contains("injected registry failure")
            );
            update_finished_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("update finished after rollback");
            update
                .join()
                .expect("update thread")
                .expect("concurrent update");
        });

        let stored = store.load(&group_id).expect("stored group");
        assert_eq!(stored.title, "concurrent");
        assert!(stored.scopes.is_empty());
        assert!(stored.active_scope_key.is_empty());
    }

    #[test]
    fn failed_attach_does_not_overwrite_a_python_style_unlocked_update() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("before", "").expect("group");
        let group_path = store
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("group.yaml");
        let scope = Scope {
            scope_key: "s_test".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "test".into(),
            git_remote: String::new(),
        };

        let error = attach_with(&store, &group.group_id, scope, |_| {
            let mut external = store.load(&group.group_id).expect("external load");
            external.title = "python concurrent".into();
            crate::fs::write_yaml(&group_path, &external).expect("external unlocked write");
            Err(io::Error::other("injected registry failure"))
        })
        .expect_err("attach failure");

        assert!(error.to_string().contains("rollback_skipped"));
        let stored = store.load(&group.group_id).expect("stored group");
        assert_eq!(stored.title, "python concurrent");
        assert_eq!(stored.active_scope_key, "s_test");
        assert_eq!(stored.scopes.len(), 1);
    }

    #[test]
    fn failed_detach_restores_group_scope_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("scopes", "").expect("group");
        let scope = Scope {
            scope_key: "s_one".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "one".into(),
            git_remote: String::new(),
        };
        let before = attach(&store, &group.group_id, scope).expect("attach");
        let error = detach_with(&store, &group.group_id, "s_one", |_| {
            Err(io::Error::other("injected registry failure"))
        })
        .expect_err("detach failure");
        assert!(error.to_string().contains("injected registry failure"));
        let stored = store.load(&group.group_id).expect("stored");
        assert_eq!(stored.scopes, before.scopes);
        assert_eq!(stored.active_scope_key, before.active_scope_key);
        assert_eq!(
            Registry::load(store.home())
                .expect("registry")
                .defaults
                .get("s_one"),
            Some(&group.group_id)
        );
    }

    #[test]
    fn failed_activate_restores_prior_active_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("scopes", "").expect("group");
        let first = Scope {
            scope_key: "s_one".into(),
            url: temp.path().join("one").to_string_lossy().into_owned(),
            label: "one".into(),
            git_remote: String::new(),
        };
        let second = Scope {
            scope_key: "s_two".into(),
            url: temp.path().join("two").to_string_lossy().into_owned(),
            label: "two".into(),
            git_remote: String::new(),
        };
        attach(&store, &group.group_id, first).expect("first");
        let before = attach(&store, &group.group_id, second).expect("second");
        let error = activate_with(&store, &group.group_id, "s_one", |_| {
            Err(io::Error::other("injected registry failure"))
        })
        .expect_err("activate failure");
        assert!(error.to_string().contains("injected registry failure"));
        let stored = store.load(&group.group_id).expect("stored");
        assert_eq!(stored.scopes, before.scopes);
        assert_eq!(stored.active_scope_key, before.active_scope_key);
        assert_eq!(
            Registry::load(store.home())
                .expect("registry")
                .defaults
                .get("s_two"),
            Some(&group.group_id)
        );
    }
}

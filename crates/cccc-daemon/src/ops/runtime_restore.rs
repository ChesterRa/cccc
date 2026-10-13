use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupStore, HomeLayout, ledger};

use crate::dispatch::OpError;
use crate::dispatch_concurrency::DispatchLocks;
use crate::ops::{actor_delivery, actor_runtime};

pub fn spawn(home: HomeLayout, locks: DispatchLocks) {
    let result = std::thread::Builder::new()
        .name("cccc-runtime-restore".into())
        .spawn(move || {
            if let Err(error) = restore_running_serialized(&home, &locks) {
                tracing::warn!(message = %error.message, "failed to restore running runtimes");
            }
        });
    if let Err(error) = result {
        tracing::warn!(%error, "failed to spawn runtime restore worker");
    }
}

#[cfg(test)]
pub fn restore_running(home: &HomeLayout) -> Result<(), OpError> {
    settle_stranded(home)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        restore_group(home, &store, &meta.group_id)?;
    }
    Ok(())
}

pub(crate) fn settle_stranded(home: &HomeLayout) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        let Ok(group) = store.load(&meta.group_id) else {
            continue;
        };
        let settled = crate::ops::runtime_delivery::settle_stranded_claims(home, &group)?;
        if settled > 0 {
            tracing::warn!(
                group_id = %meta.group_id,
                settled,
                "settled stranded runtime delivery claims before daemon IPC startup"
            );
        }
    }
    Ok(())
}

fn restore_running_serialized(home: &HomeLayout, locks: &DispatchLocks) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        locks.with_group_write_blocking(&meta.group_id, || {
            restore_group(home, &store, &meta.group_id)
        })?;
    }
    Ok(())
}

fn restore_group(home: &HomeLayout, store: &GroupStore, group_id: &str) -> Result<(), OpError> {
    let Ok(mut group) = store.load(group_id) else {
        return Ok(());
    };
    if cccc_core::group_scope::normalize_actor_scope_keys(&mut group) > 0 {
        group = store
            .mutate(group_id, |current| {
                cccc_core::group_scope::normalize_actor_scope_keys(current);
                Ok(current.clone())
            })
            .map_err(OpError::io)?;
    }
    if !group.running || group.state == cccc_contracts::GroupState::Stopped {
        return Ok(());
    }
    for actor in group
        .actors
        .iter()
        .filter(|actor| should_restore_actor(group.state, actor))
    {
        if deepseek_restore_blocked(home, &group, actor) {
            tracing::info!(
                group_id = %group.group_id,
                actor_id = %actor.id,
                "skipped automatic DeepSeek restore until an explicit actor start"
            );
            continue;
        }
        let was_running = actor_runtime::actor_is_running(&group, actor);
        match actor_runtime::apply(home, &group, &actor.id, "actor.restore") {
            Ok(_) => {
                if !was_running && actor_runtime::actor_is_running(&group, actor) {
                    record_respawn_event(store, &group, &actor.id);
                }
                actor_delivery::dispatch_unread(home, &group, &actor.id);
            }
            Err(error) => {
                tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %error.message,
                    "failed to restore actor runtime"
                );
            }
        }
    }
    Ok(())
}

fn should_restore_actor(state: GroupState, actor: &Actor) -> bool {
    actor.enabled
        && !(state == GroupState::Paused && actor.runner == cccc_contracts::RunnerKind::Headless)
}

/// Record a newly running daemon-owned Actor after startup restoration.
fn record_respawn_event(store: &GroupStore, group: &cccc_core::GroupDoc, actor_id: &str) {
    let mut event = Event::new("actor.start", &group.group_id);
    event.by = "system".into();
    event.data = serde_json::json!({
        "actor_id": actor_id,
        "reason": "daemon_respawn",
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    if let Err(error) = store
        .ledger_path(&group.group_id)
        .and_then(|path| ledger::append(&path, &event))
    {
        tracing::warn!(
            group_id = %group.group_id,
            actor_id = %actor_id,
            message = %error,
            "failed to record daemon respawn actor.start event"
        );
    }
}

fn deepseek_restore_blocked(home: &HomeLayout, group: &cccc_core::GroupDoc, actor: &Actor) -> bool {
    actor.runtime == cccc_contracts::ActorRuntime::Deepseek
        && crate::ops::deepseek_runtime::manual_restart_required(home, group, actor)
}

#[cfg(test)]
mod tests {
    use super::{deepseek_restore_blocked, restore_group, should_restore_actor};
    use crate::ops::actor_runtime;
    use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};
    use cccc_core::{GroupDoc, GroupStore, HomeLayout, Scope, ledger};

    #[test]
    fn paused_groups_restore_terminal_runtimes_but_not_non_terminal_runtimes() {
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Deepseek;
        actor.normalize_runtime_constraints();
        assert!(!should_restore_actor(GroupState::Paused, &actor));
        assert!(should_restore_actor(GroupState::Active, &actor));

        actor.runtime = ActorRuntime::Claude;
        actor.normalize_runtime_constraints();
        assert!(should_restore_actor(GroupState::Paused, &actor));
    }

    #[test]
    fn durable_deepseek_gate_blocks_automatic_restore_until_a_new_generation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("deepseek restore", "").expect("group");
        let mut actor = Actor::new("deepseek");
        actor.runtime = ActorRuntime::Deepseek;
        group.actors.push(actor.clone());
        store.save(&group).expect("save");

        cccc_core::deepseek_restart_gate::record_running_generation(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            "launch-1",
        )
        .expect("record generation");
        cccc_core::deepseek_restart_gate::require_manual_restart(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            "launch-1",
            "credential_unavailable",
        )
        .expect("close gate");
        assert!(deepseek_restore_blocked(&home, &group, &actor));

        cccc_core::deepseek_restart_gate::record_running_generation(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            "launch-2",
        )
        .expect("record explicit restart generation");
        assert!(!deepseek_restore_blocked(&home, &group, &actor));
    }

    fn restore_fixture(
        runtime: ActorRuntime,
    ) -> (tempfile::TempDir, HomeLayout, GroupStore, GroupDoc) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let workspace = temp.path().join("workspace");
        let provider_home = temp.path().join("provider-home");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::create_dir(&provider_home).expect("provider home");
        let mut group = store.create("restore test", "").expect("group");
        group.scopes.push(Scope {
            scope_key: "s_restore".into(),
            url: workspace.to_string_lossy().into_owned(),
            label: "restore".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "s_restore".into();
        let mut actor = Actor::new("peer1");
        actor.runtime = runtime;
        actor.runner = if runtime == ActorRuntime::Custom {
            RunnerKind::Pty
        } else {
            RunnerKind::Headless
        };
        actor.default_scope_key = "s_restore".into();
        actor.enabled = true;
        if runtime == ActorRuntime::Custom {
            actor.command = vec![
                "sh".into(),
                "-c".into(),
                "while read line; do :; done".into(),
            ];
        }
        for key in ["HOME", "USERPROFILE"] {
            actor
                .env
                .insert(key.into(), provider_home.to_string_lossy().into_owned());
        }
        group.actors.push(actor);
        group.running = true;
        group.state = GroupState::Active;
        store.save(&group).expect("save");
        (temp, home, store, group)
    }

    fn respawn_events(store: &GroupStore, group: &GroupDoc) -> Vec<cccc_contracts::Event> {
        ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("read ledger")
            .into_iter()
            .filter(|event| event.kind == "actor.start" && event.data["reason"] == "daemon_respawn")
            .collect()
    }

    #[test]
    fn structured_restore_without_a_local_owner_does_not_record_a_respawn() {
        for runtime in [ActorRuntime::WebModel, ActorRuntime::ChatgptDots] {
            let (_temp, home, store, group) = restore_fixture(runtime);
            assert!(!actor_runtime::actor_is_running(&group, &group.actors[0]));
            restore_group(&home, &store, &group.group_id).expect("restore group");
            assert!(!actor_runtime::actor_is_running(&group, &group.actors[0]));
            assert!(respawn_events(&store, &group).is_empty());
        }
    }

    #[cfg(unix)]
    struct OwnedRuntime(String);

    #[cfg(unix)]
    impl Drop for OwnedRuntime {
        fn drop(&mut self) {
            let _ = cccc_runtime::stop(&self.0, "peer1");
        }
    }

    #[cfg(unix)]
    #[test]
    fn restored_owner_records_one_respawn_and_healthy_restore_is_idempotent() {
        let (_temp, home, store, group) = restore_fixture(ActorRuntime::Custom);
        let _cleanup = OwnedRuntime(group.group_id.clone());
        restore_group(&home, &store, &group.group_id).expect("first restore");
        let first = actor_runtime::status(&group.group_id, "peer1").expect("owned runtime");
        assert!(first.running && first.pid.is_some());
        let events = respawn_events(&store, &group);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data["actor_id"], "peer1");
        assert_eq!(events[0].by, "system");

        restore_group(&home, &store, &group.group_id).expect("idempotent restore");
        let next = actor_runtime::status(&group.group_id, "peer1").expect("same owned runtime");
        assert!(next.running);
        assert_eq!(next.pid, first.pid);
        assert_eq!(next.started_at, first.started_at);
        assert_eq!(respawn_events(&store, &group).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn failed_restore_does_not_record_a_respawn() {
        let (_temp, home, store, mut group) = restore_fixture(ActorRuntime::Custom);
        let _cleanup = OwnedRuntime(group.group_id.clone());
        group.active_scope_key.clear();
        group.actors[0].default_scope_key.clear();
        group.scopes.clear();
        store.save(&group).expect("save missing scope");
        restore_group(&home, &store, &group.group_id).expect("failed actor restore is nonfatal");
        assert!(!actor_runtime::actor_is_running(&group, &group.actors[0]));
        assert!(respawn_events(&store, &group).is_empty());
    }
}

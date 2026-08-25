use cccc_contracts::{Actor, GroupState};
use cccc_core::{GroupStore, HomeLayout};

use crate::dispatch::OpError;
use crate::dispatch_concurrency::DispatchLocks;
use crate::ops::{actor_delivery, actor_runtime, local_headless};

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
        match actor_runtime::apply(home, &group, &actor.id, "actor.start") {
            Ok(_) => {
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
    actor.enabled && !(state == GroupState::Paused && local_headless::supports(actor))
}

fn deepseek_restore_blocked(home: &HomeLayout, group: &cccc_core::GroupDoc, actor: &Actor) -> bool {
    actor.runtime == cccc_contracts::ActorRuntime::Deepseek
        && crate::ops::deepseek_runtime::manual_restart_required(home, group, actor)
}

#[cfg(test)]
mod tests {
    use super::{deepseek_restore_blocked, should_restore_actor};
    use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};
    use cccc_core::{GroupStore, HomeLayout};

    #[test]
    fn paused_groups_restore_retained_ptys_but_not_headless_runtimes() {
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Claude;
        actor.runner = RunnerKind::Headless;
        assert!(!should_restore_actor(GroupState::Paused, &actor));
        assert!(should_restore_actor(GroupState::Active, &actor));

        actor.runner = RunnerKind::Pty;
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
}

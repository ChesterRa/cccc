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
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        restore_group(home, &store, &meta.group_id)?;
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
        match actor_runtime::apply(home, &group, &actor.id, "actor.start") {
            Ok(_) => {
                if actor.runtime == cccc_contracts::ActorRuntime::Deepseek {
                    let recovered = crate::ops::deepseek_runtime::recover(home, &group, actor, 256);
                    if recovered > 0 {
                        tracing::info!(
                            group_id = %group.group_id,
                            actor_id = %actor.id,
                            recovered,
                            "recovered durable DeepSeek terminal prefix"
                        );
                    }
                }
                actor_delivery::dispatch_unread_notice(home, &group, &actor.id);
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

#[cfg(test)]
mod tests {
    use super::should_restore_actor;
    use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};

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
}

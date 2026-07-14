use cccc_contracts::ActorRuntime;
use cccc_core::{GroupStore, HomeLayout};

use crate::dispatch::OpError;
use crate::ops::actor_runtime;

pub fn restore_running(home: &HomeLayout) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        let Ok(group) = store.load(&meta.group_id) else {
            continue;
        };
        if !group.running || group.state == cccc_contracts::GroupState::Stopped {
            continue;
        }
        for actor in group
            .actors
            .iter()
            .filter(|actor| actor.enabled && actor.runtime != ActorRuntime::WebModel)
        {
            if let Err(error) = actor_runtime::apply(home, &group, &actor.id, "actor.start") {
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

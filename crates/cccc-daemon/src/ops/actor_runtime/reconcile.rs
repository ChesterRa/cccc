use cccc_contracts::{Event, RuntimeStateSource};
use cccc_core::ledger;
use cccc_core::{GroupStore, HomeLayout};
use cccc_runtime::SessionStatus;

use super::runtime_error;
use crate::dispatch::OpError;

pub fn reap_exited() -> Result<Vec<SessionStatus>, OpError> {
    cccc_runtime::reap().map_err(runtime_error)
}

pub fn reconcile_exited(home: &HomeLayout, exited: Vec<SessionStatus>) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for status in exited {
        let Ok(group) = store.load(&status.group_id) else {
            continue;
        };
        let Some(actor) = group
            .actors
            .iter()
            .find(|actor| actor.id == status.actor_id)
        else {
            continue;
        };
        if actor.runtime_state_source != RuntimeStateSource::Terminal {
            continue;
        }
        store
            .mutate(&status.group_id, |doc| {
                if let Some(actor) = doc
                    .actors
                    .iter_mut()
                    .find(|actor| actor.id == status.actor_id)
                {
                    actor.enabled = false;
                }
                doc.running = doc.actors.iter().any(|actor| actor.enabled);
                Ok(())
            })
            .map_err(OpError::io)?;
        append_exit_event(&store, status)?;
    }
    Ok(())
}

fn append_exit_event(store: &GroupStore, status: SessionStatus) -> Result<(), OpError> {
    let mut event = Event::new("actor.stop", &status.group_id);
    event.by = "system".into();
    event.data = serde_json::json!({
        "actor_id": status.actor_id,
        "reason": "process_exit",
        "exit_code": status.exit_code,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    ledger::append(
        &store.ledger_path(&status.group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}

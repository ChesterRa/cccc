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
            append_exit_event(&store, status)?;
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

#[cfg(test)]
mod tests {
    use cccc_contracts::{Actor, RunnerKind, RuntimeStateSource};
    use cccc_core::{GroupStore, HomeLayout, ledger};
    use cccc_runtime::SessionStatus;

    use super::reconcile_exited;

    #[test]
    fn app_server_exit_is_recorded_without_disabling_actor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("test", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                let mut actor = Actor::new("peer1");
                actor.runtime_state_source = RuntimeStateSource::AppServer;
                doc.actors.push(actor);
                doc.running = true;
                Ok(())
            })
            .expect("add actor");

        let result = reconcile_exited(
            &home,
            vec![SessionStatus {
                group_id: group.group_id.clone(),
                actor_id: "peer1".into(),
                runner: RunnerKind::Pty,
                running: false,
                pid: Some(42),
                started_at: "2026-07-27T00:00:00Z".into(),
                exit_code: Some(7),
            }],
        );
        assert!(result.is_ok());

        let reloaded = store.load(&group.group_id).expect("reload group");
        assert!(reloaded.actors[0].enabled);
        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("read ledger");
        let event = events.last().expect("exit event");
        assert_eq!(event.kind, "actor.stop");
        assert_eq!(event.data["actor_id"], "peer1");
        assert_eq!(event.data["exit_code"], 7);
    }
}

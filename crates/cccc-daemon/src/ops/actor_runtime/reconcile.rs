use cccc_contracts::Event;
use cccc_core::ledger;
use cccc_core::{GroupStore, HomeLayout};
use cccc_runtime::SessionStatus;

use super::runtime_error;
use crate::dispatch::OpError;

pub fn reap_exited() -> Result<Vec<SessionStatus>, OpError> {
    cccc_runtime::reap()
        .map(reconciliable_exits)
        .map_err(runtime_error)
}

fn reconciliable_exits(exited: Vec<SessionStatus>) -> Vec<SessionStatus> {
    exited
        .into_iter()
        .filter(|status| {
            super::super::runtime_hook_session::with_launch_lock(
                &status.group_id,
                &status.actor_id,
                || {
                    !super::resume_verification::is_monitoring(status)
                        && !has_running_replacement(status)
                },
            )
        })
        .collect()
}

fn has_running_replacement(status: &SessionStatus) -> bool {
    cccc_runtime::status(&status.group_id, &status.actor_id).is_ok_and(|current| current.running)
}

pub fn reconcile_exited(home: &HomeLayout, exited: Vec<SessionStatus>) -> Result<(), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for status in exited {
        let group_id = status.group_id.clone();
        let actor_id = status.actor_id.clone();
        super::super::runtime_hook_session::with_launch_lock(&group_id, &actor_id, || {
            reconcile_one(&store, status)
        })?;
    }
    Ok(())
}

fn reconcile_one(store: &GroupStore, status: SessionStatus) -> Result<(), OpError> {
    if has_running_replacement(&status) {
        return Ok(());
    }
    super::super::runtime_hook_session::revoke(&status.group_id, &status.actor_id);
    super::super::runtime_hook_input::reset(&status.group_id, &status.actor_id);
    let Ok(group) = store.load(&status.group_id) else {
        return Ok(());
    };
    if !group.actors.iter().any(|actor| actor.id == status.actor_id) {
        return Ok(());
    }
    // Preserve desired lifecycle after a provider exit. A later user-directed
    // message follows the same wake path whether the process exited or was stopped.
    append_exit_event(store, status)
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
    use cccc_runtime::{LaunchSpec, SessionStatus};
    use std::collections::BTreeMap;

    use super::{reconcile_exited, reconciliable_exits};

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

    #[test]
    fn terminal_exit_preserves_desired_lifecycle_for_message_auto_wake() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("recoverable terminal", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                doc.actors.push(Actor::new("peer1"));
                doc.running = true;
                Ok(())
            })
            .expect("add actor");

        reconcile_exited(
            &home,
            vec![SessionStatus {
                group_id: group.group_id.clone(),
                actor_id: "peer1".into(),
                runner: RunnerKind::Pty,
                running: false,
                pid: Some(42),
                started_at: "2026-08-25T00:00:00Z".into(),
                exit_code: Some(1),
            }],
        )
        .expect("reconcile terminal exit");

        let reloaded = store.load(&group.group_id).expect("reload group");
        assert!(reloaded.running);
        assert!(reloaded.actors[0].enabled);
        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("read ledger");
        let event = events.last().expect("exit event");
        assert_eq!(event.kind, "actor.stop");
        assert_eq!(event.by, "system");
        assert_eq!(event.data["reason"], "process_exit");
    }

    #[test]
    fn resume_verifier_owns_early_exit_reconciliation() {
        let status = SessionStatus {
            group_id: "g_resume_verification".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Pty,
            running: false,
            pid: Some(42),
            started_at: "2026-08-07T00:00:00Z".into(),
            exit_code: Some(1),
        };
        let _registration = super::super::resume_verification::register_for_test(&status);

        assert!(reconciliable_exits(vec![status.clone()]).is_empty());
    }

    #[test]
    fn stale_exit_does_not_disable_a_running_replacement() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("replacement", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                doc.actors.push(Actor::new("peer1"));
                doc.running = true;
                Ok(())
            })
            .expect("add actor");
        let current = cccc_runtime::start(LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "sleep 30".into()],
            cwd: temp.path().to_path_buf(),
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        })
        .expect("replacement runtime");

        reconcile_exited(
            &home,
            vec![SessionStatus {
                group_id: group.group_id.clone(),
                actor_id: "peer1".into(),
                runner: RunnerKind::Pty,
                running: false,
                pid: Some(41),
                started_at: "older-session".into(),
                exit_code: Some(1),
            }],
        )
        .expect("reconcile stale exit");

        let reloaded = store.load(&group.group_id).expect("reload group");
        assert!(reloaded.running);
        assert!(reloaded.actors[0].enabled);
        assert_eq!(
            cccc_runtime::status(&group.group_id, "peer1")
                .expect("current runtime")
                .started_at,
            current.started_at
        );
        cccc_runtime::stop(&group.group_id, "peer1").expect("cleanup");
    }
}

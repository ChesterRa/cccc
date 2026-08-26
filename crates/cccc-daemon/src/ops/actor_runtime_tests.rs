use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, Scope, actors, inbox, ledger};
use serde_json::{Map, json};

use crate::dispatch_concurrency::DispatchLocks;

use super::{actor_delivery, actor_runtime, runtime_restore};

#[test]
fn restore_migrates_legacy_actor_scope_paths_even_for_stopped_groups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("restore migration", "").expect("group");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    store
        .mutate(&group.group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: project.to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.default_scope_key = project.to_string_lossy().into_owned();
            actors::add(group, actor)
        })
        .expect("legacy actor");

    runtime_restore::restore_running(&home).expect("restore");

    let stored = store.load(&group.group_id).expect("stored group");
    assert_eq!(stored.actors[0].default_scope_key, "s_project");
}

#[test]
fn pty_actor_start_without_a_scope_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("missing scope", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Custom;
            actor.runner = RunnerKind::Pty;
            actor.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
            actors::add(group, actor)
        })
        .expect("actor");

    let response = request(&home, "actor_start", &group_id);
    assert!(!response.ok);
    assert_eq!(
        response.error.expect("missing scope error").code,
        "missing_project_root"
    );
    assert!(actor_runtime::status(&group_id, "peer1").is_none());
    let stored = store.load(&group_id).expect("stored group");
    assert!(!stored.running);
    assert_eq!(stored.state, GroupState::Active);
}

#[test]
fn restores_enabled_actors_for_persisted_running_groups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("restore", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: temp.path().to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.default_scope_key = "s_detached".into();
            actor.runtime = ActorRuntime::Custom;
            actor.runner = RunnerKind::Pty;
            actor.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
            actors::add(group, actor)?;
            group.running = true;
            group.state = GroupState::Active;
            Ok(())
        })
        .expect("configure group");

    assert!(runtime_restore::restore_running(&home).is_ok());
    assert!(actor_runtime::status(&group_id, "peer1").is_some_and(|status| status.running));
    assert_eq!(
        store.load(&group_id).expect("restored group").actors[0].default_scope_key,
        "s_project"
    );
    cccc_runtime::stop(&group_id, "peer1").expect("stop");
}

#[cfg(unix)]
#[tokio::test]
async fn detached_restore_waits_for_the_group_mutation_lock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("serialized restore", "").expect("group");
    let group_id = group.group_id.clone();
    let marker = temp.path().join("restored-while-locked");
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: temp.path().to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Custom;
            actor.runner = RunnerKind::Pty;
            actor.command = vec![
                "sh".into(),
                "-c".into(),
                ": > \"$CCCC_TEST_RESTORE_MARKER\"; sleep 2".into(),
            ];
            actor.env.insert(
                "CCCC_TEST_RESTORE_MARKER".into(),
                marker.to_string_lossy().into_owned(),
            );
            actors::add(group, actor)?;
            group.running = true;
            group.state = GroupState::Active;
            Ok(())
        })
        .expect("configure group");

    let locks = DispatchLocks::default();
    let permit = locks.group_write(&group_id).await;
    runtime_restore::spawn(home.clone(), locks.clone());
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert!(
        !marker.exists(),
        "detached restore bypassed an in-progress group mutation"
    );
    drop(permit);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        marker.exists(),
        "restore did not resume after the group lock"
    );
    cccc_runtime::stop(&group_id, "peer1").expect("stop");
}

#[test]
fn restore_recovers_one_pending_send_without_advancing_read_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("restore unread", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: temp.path().to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Custom;
            actor.runner = RunnerKind::Pty;
            actor.submit = cccc_contracts::ActorSubmit::Newline;
            actor.command = vec![
                "sh".into(),
                "-c".into(),
                "stty -echo; IFS= read -r preamble; IFS= read -r message; printf 'RESTORED:%s' \"$message\"; sleep 2".into(),
            ];
            actors::add(group, actor)?;
            group.running = true;
            group.state = GroupState::Active;
            Ok(())
        })
        .expect("configure group");
    let mut event = cccc_contracts::Event::new("chat.message", &group_id);
    event.by = "user".into();
    event.data = json!({"to":["peer1"],"text":"message-before-restart","message_mode":"send"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(&store.ledger_path(&group_id).expect("ledger"), &event).expect("message");

    runtime_restore::restore_running(&home).expect("restore");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let events =
            ledger::read_all(&store.ledger_path(&group_id).expect("ledger")).expect("events");
        if events.iter().any(|delivery| {
            delivery.kind == "runtime.delivery"
                && delivery.data["source_event_id"] == event.id
                && delivery.data["actor_id"] == "peer1"
                && delivery.data["state"] == "accepted"
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "restored actor did not accept the pending Send"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        inbox::cursor(&home, &group_id, "peer1")
            .expect("cursor")
            .is_none()
    );

    actor_delivery::shutdown_actor(&group_id, "peer1");
    let _ = cccc_runtime::stop(&group_id, "peer1");
}

#[test]
fn relaunch_preserves_runtime_metadata_but_new_session_clears_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("session lifecycle", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: temp.path().to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Codex;
            actor.runner = RunnerKind::Pty;
            actor.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
            actors::add(group, actor)
        })
        .expect("actor");
    let session_path = store
        .state_dir(&group_id)
        .expect("state")
        .join("runtime_sessions/peer1.json");
    cccc_core::fs::write_json(
        &session_path,
        &json!({
            "runtime":"codex",
            "status":"usable",
            "resume_eligible":true,
            "provider_session_id":"019eece8-8c6d-7811-a700-26593825ae2d"
        }),
    )
    .expect("metadata");

    assert!(request(&home, "actor_restart", &group_id).ok);
    assert!(session_path.is_file());

    assert!(request(&home, "actor_new_session", &group_id).ok);
    assert!(!session_path.exists());
    cccc_runtime::stop(&group_id, "peer1").expect("stop");
}

#[test]
fn new_session_ledger_failure_restores_runtime_metadata_and_stops_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("session rollback", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: temp.path().to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Codex;
            actor.runner = RunnerKind::Pty;
            actor.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
            actors::add(group, actor)
        })
        .expect("actor");
    let session_path = store
        .state_dir(&group_id)
        .expect("state")
        .join("runtime_sessions/peer1.json");
    cccc_core::fs::write_json(
        &session_path,
        &json!({
            "runtime":"codex",
            "status":"usable",
            "resume_eligible":true,
            "provider_session_id":"019eece8-8c6d-7811-a700-26593825ae2d"
        }),
    )
    .expect("metadata");
    let ledger_path = store.ledger_path(&group_id).expect("ledger");
    std::fs::remove_file(&ledger_path).expect("remove ledger");
    std::fs::create_dir(&ledger_path).expect("block ledger append");

    let response = request(&home, "actor_new_session", &group_id);

    assert!(
        !response.ok,
        "corrupt ledger unexpectedly accepted new session"
    );
    let restored: serde_json::Value =
        cccc_core::fs::read_json(&session_path).expect("restored metadata");
    assert_eq!(
        restored["provider_session_id"],
        "019eece8-8c6d-7811-a700-26593825ae2d"
    );
    assert!(
        actor_runtime::status(&group_id, "peer1").is_none_or(|status| !status.running),
        "replacement runtime survived failed commit"
    );
}

pub(super) fn request(
    home: &HomeLayout,
    op: &str,
    group_id: &str,
) -> cccc_contracts::DaemonResponse {
    crate::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: Map::from_iter([
                ("group_id".into(), json!(group_id)),
                ("actor_id".into(), json!("peer1")),
                ("by".into(), json!("user")),
            ]),
        },
    )
}

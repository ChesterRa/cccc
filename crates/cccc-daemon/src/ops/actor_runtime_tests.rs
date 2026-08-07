use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, Scope, actors};
use serde_json::{Map, json};

use super::{actor_runtime, runtime_restore};

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

fn request(home: &HomeLayout, op: &str, group_id: &str) -> cccc_contracts::DaemonResponse {
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

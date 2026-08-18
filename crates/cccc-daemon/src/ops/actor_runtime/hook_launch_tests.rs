use super::hook_launch::{self, LaunchIntegration, launch_integration};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};
use cccc_core::{GroupStore, HomeLayout, actors};
use std::sync::{Arc, Barrier};

#[test]
fn codex_app_server_uses_lifecycle_hooks_for_working_state() {
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Codex;
    actor.runtime_state_source = RuntimeStateSource::AppServer;
    assert_eq!(launch_integration(&actor), LaunchIntegration::CodexHooks);

    actor.runtime_state_source = RuntimeStateSource::Terminal;
    assert_eq!(launch_integration(&actor), LaunchIntegration::CodexHooks);

    actor.runtime = ActorRuntime::Claude;
    actor.runtime_state_source = RuntimeStateSource::AppServer;
    assert_eq!(launch_integration(&actor), LaunchIntegration::None);
}

#[test]
fn concurrent_launches_keep_process_identity_and_hook_token_aligned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("launch", "").expect("group");
    let actor = store
        .mutate(&group.group_id, |doc| {
            let mut actor = Actor::new("peer");
            actor.runtime = ActorRuntime::Codex;
            actor.runner = RunnerKind::Pty;
            actor.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
            actors::add(doc, actor)
        })
        .expect("actor");
    let group = store.load(&group.group_id).expect("reload group");
    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|_| {
            let home = home.clone();
            let group = group.clone();
            let actor = actor.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                hook_launch::launch(
                    &home,
                    &group,
                    &actor,
                    temp_dir(&home).as_path(),
                    &Default::default(),
                    actor.command.clone(),
                )
                .expect("launch")
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let statuses = threads
        .into_iter()
        .map(|thread| thread.join().expect("join"))
        .collect::<Vec<_>>();
    assert_eq!(statuses[0].pid, statuses[1].pid);
    assert_eq!(statuses[0].started_at, statuses[1].started_at);

    let identity = cccc_core::runtime_hook_identity::read(&home, &group.group_id, &actor.id)
        .expect("identity");
    let state =
        cccc_core::codex_hook_state::read(&home, &group.group_id, &actor.id).expect("state");
    assert_eq!(identity.pid, statuses[0].pid.expect("pid"));
    assert_eq!(identity.launch_token, state.launch_token);
    assert!(!identity.hook_enabled);
    cccc_runtime::stop(&group.group_id, &actor.id).expect("stop");
}

fn temp_dir(home: &HomeLayout) -> std::path::PathBuf {
    home.root().to_path_buf()
}

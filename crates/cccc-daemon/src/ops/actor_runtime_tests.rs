use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, actors};

use super::{actor_runtime, runtime_restore};

#[test]
fn restores_enabled_actors_for_persisted_running_groups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("restore", "").expect("group");
    let group_id = group.group_id.clone();
    store
        .mutate(&group_id, |group| {
            let mut actor = Actor::new("peer1");
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
    cccc_runtime::stop(&group_id, "peer1").expect("stop");
}

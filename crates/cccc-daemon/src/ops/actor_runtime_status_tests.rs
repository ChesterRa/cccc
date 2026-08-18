use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::Map;

use super::{actor_activity, actor_listing, actor_runtime_status, group_runtime};

#[test]
fn stopped_deepseek_supervisor_is_stopped_in_every_runtime_projection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let created = store.create("deepseek projection", "").expect("group");
    store
        .mutate(&created.group_id, |group| {
            group.running = true;
            group.state = GroupState::Active;
            let mut actor = Actor::new("deepseek");
            actor.runtime = ActorRuntime::Deepseek;
            actor.runner = RunnerKind::Headless;
            group.actors.push(actor);
            Ok(())
        })
        .expect("deepseek actor");
    let group = store.load(&created.group_id).expect("stored group");
    let actor = &group.actors[0];

    assert!(!actor_runtime_status::resolve(&group, actor).running);
    assert_eq!(group_runtime::status(&group)["runtime_running"], false);
    assert_eq!(
        group_runtime::group(group.clone())["actors"][0]["running"],
        false
    );
    assert!(actor_activity::running_payload(&home, &group, actor).is_none());

    let request = DaemonRequest {
        v: 1,
        op: "actor_list".into(),
        args: Map::new(),
    };
    let actors = actor_listing::list(&home, &group, &request).expect("actor projection");
    assert_eq!(actors[0]["running"], false);
}

use crate::{GroupStore, HomeLayout, actors};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use serde_json::{Map, json};

#[test]
fn deepseek_add_and_update_persist_headless_runner() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let mut group = store.create("deepseek normalization", "").expect("group");

    let mut deepseek = Actor::new("deepseek");
    deepseek.runtime = ActorRuntime::Deepseek;
    deepseek.runner = RunnerKind::Pty;
    let added = actors::add(&mut group, deepseek).expect("add");
    assert_eq!(added.runner, RunnerKind::Headless);
    assert_eq!(group.actors[0].runner, RunnerKind::Headless);

    let mut custom = Actor::new("custom");
    custom.runtime = ActorRuntime::Custom;
    actors::add(&mut group, custom).expect("custom add");
    let patch = Map::from_iter([
        ("runtime".into(), json!("deepseek")),
        ("runner".into(), json!("pty")),
    ]);
    let updated = actors::update(&mut group, "custom", &patch).expect("update");
    assert_eq!(updated.runtime, ActorRuntime::Deepseek);
    assert_eq!(updated.runner, RunnerKind::Headless);
}

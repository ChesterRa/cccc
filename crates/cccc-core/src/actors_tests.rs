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

#[test]
fn repeated_runtime_preserves_acp_mode_but_changed_runtime_resets_it() {
    use cccc_contracts::RuntimeMode;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let mut group = GroupStore::new(home)
        .expect("store")
        .create("mode", "")
        .expect("group");
    let mut actor = Actor::new("worker");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runtime_mode = RuntimeMode::Acp;
    actors::add(&mut group, actor).expect("add");
    let same = json!({"runtime":"antigravity","command":["agy","--model","fixture"]});
    let updated =
        actors::update(&mut group, "worker", same.as_object().expect("patch")).expect("update");
    assert_eq!(updated.runtime_mode, RuntimeMode::Acp);
    assert_eq!(updated.runner, RunnerKind::Headless);
    let changed = json!({"runtime":"codex"});
    let updated =
        actors::update(&mut group, "worker", changed.as_object().expect("patch")).expect("change");
    assert_eq!(updated.runtime_mode, RuntimeMode::Default);
}

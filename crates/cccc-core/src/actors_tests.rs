use crate::{GroupStore, HomeLayout, actors};
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use serde_json::{Map, json};

#[test]
fn chet_provenance_requires_current_group_and_ledger_membership() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Chet provenance", "").expect("group");
    let mut message = cccc_contracts::Event::new("chat.message", &group.group_id);
    message.by = actors::CHET_SERVICE_PRINCIPAL.into();
    message.data.insert(
        cccc_contracts::chet::GROUP_AUTHOR_GENERATION_FIELD.into(),
        json!(crate::connect_groups::generation(&group)),
    );

    // The predicate is suitable for an already-held ledger snapshot. By
    // itself it does not make caller-supplied data a trusted ledger event.
    assert!(actors::has_current_chet_provenance(&group, &message));
    assert!(!actors::is_chet_message(&home, &group, &message).expect("not in ledger"));
    crate::ledger::append(&store.ledger_path(&group.group_id).expect("path"), &message)
        .expect("append");
    assert!(actors::is_chet_message(&home, &group, &message).expect("in ledger"));

    let mut changed = message.clone();
    changed.by = "worker".into();
    assert!(!actors::has_current_chet_provenance(&group, &changed));
    changed = message.clone();
    changed.group_id = "g_other".into();
    assert!(!actors::has_current_chet_provenance(&group, &changed));
    changed = message.clone();
    changed.kind = "group.create".into();
    assert!(!actors::has_current_chet_provenance(&group, &changed));
    changed = message.clone();
    changed.data.insert(
        cccc_contracts::chet::GROUP_AUTHOR_GENERATION_FIELD.into(),
        json!("previous-generation"),
    );
    assert!(!actors::has_current_chet_provenance(&group, &changed));
    changed = message.clone();
    changed.data.insert("text".into(), json!("tampered"));
    assert!(actors::has_current_chet_provenance(&group, &changed));
    assert!(!actors::is_chet_message(&home, &group, &changed).expect("different event"));
}

#[test]
fn chet_selector_refuses_existing_member_id_or_title_collisions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let mut group = GroupStore::new(home)
        .expect("store")
        .create("selectors", "")
        .expect("group");
    assert!(actors::is_chet_selector(&group, "@chet").expect("service"));
    assert!(actors::is_chet_selector(&group, " @CHET ").expect("case-insensitive service"));
    assert!(!actors::is_chet_selector(&group, "chet").expect("ordinary token"));
    assert!(!actors::is_chet_selector(&group, "@foreman").expect("ordinary selector"));

    let mut member = Actor::new("chet");
    member.title = "other".into();
    actors::add(&mut group, member).expect("existing member ID");
    let conflict = actors::is_chet_selector(&group, "@chet").expect_err("ID collision");
    assert!(conflict.to_string().contains("panel"));
    assert_eq!(
        actors::resolve_recipients(&group, &["chet".into()]).expect("ordinary ID"),
        vec!["chet"]
    );

    group.actors.clear();
    let mut member = Actor::new("worker");
    member.title = "ChEt".into();
    actors::add(&mut group, member).expect("existing member title");
    let conflict = actors::is_chet_selector(&group, "@chet").expect_err("title collision");
    assert!(conflict.to_string().contains("title"));
    assert_eq!(
        actors::resolve_recipients(&group, &["worker".into()]).expect("ordinary ID"),
        vec!["worker"]
    );
}

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

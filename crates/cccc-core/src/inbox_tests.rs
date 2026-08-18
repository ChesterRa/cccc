use crate::{GroupStore, HomeLayout, inbox, ledger};
use cccc_contracts::{Actor, ActorRuntime, Event};

#[test]
fn deepseek_cursor_gap_check_starts_at_current_actor_generation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("generation gap", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");

    let mut old_broadcast = Event::new("chat.message", &group.group_id);
    old_broadcast.by = "user".into();
    old_broadcast.data = serde_json::json!({"text":"before actor creation"})
        .as_object()
        .cloned()
        .expect("data");
    ledger::append(&ledger_path, &old_broadcast).expect("old broadcast");

    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    group.actors.push(actor.clone());
    store.save(&group).expect("save actor");
    let mut actor_add = Event::new("actor.add", &group.group_id);
    actor_add.by = "user".into();
    actor_add.data = serde_json::json!({"actor": actor})
        .as_object()
        .cloned()
        .expect("actor data");
    ledger::append(&ledger_path, &actor_add).expect("actor add");

    let mut first = Event::new("chat.message", &group.group_id);
    first.by = "user".into();
    first.data = serde_json::json!({"to":["deepseek"],"text":"first"})
        .as_object()
        .cloned()
        .expect("first data");
    ledger::append(&ledger_path, &first).expect("first");
    assert!(inbox::advance(&home, &group.group_id, "deepseek", &first.id).expect("advance"));

    let mut second = Event::new("chat.message", &group.group_id);
    second.by = "user".into();
    second.data = serde_json::json!({"to":["deepseek"],"text":"second"})
        .as_object()
        .cloned()
        .expect("second data");
    ledger::append(&ledger_path, &second).expect("second");
    let mut third = Event::new("chat.message", &group.group_id);
    third.by = "user".into();
    third.data = serde_json::json!({"to":["deepseek"],"text":"third"})
        .as_object()
        .cloned()
        .expect("third data");
    ledger::append(&ledger_path, &third).expect("third");
    let error = inbox::advance(&home, &group.group_id, "deepseek", &third.id)
        .expect_err("cannot skip second");
    assert!(error.to_string().contains("cannot skip"));

    // Duplicate or delayed acknowledgements are harmless and must not slice
    // the ledger with a reversed range after the cursor already advanced.
    assert!(
        !inbox::advance(&home, &group.group_id, "deepseek", &first.id).expect("duplicate advance")
    );
}

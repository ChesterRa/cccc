use crate::{GroupStore, HomeLayout, inbox, ledger};
use cccc_contracts::{Actor, Event};

#[test]
fn mail_cursor_is_generation_bounded_and_can_advance_to_later_mail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("generation gap", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");

    let mut old_broadcast = Event::new("chat.message", &group.group_id);
    old_broadcast.by = "user".into();
    old_broadcast.data = serde_json::json!({"text":"before actor creation","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("data");
    ledger::append(&ledger_path, &old_broadcast).expect("old broadcast");

    let actor = Actor::new("peer1");
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
    first.data = serde_json::json!({"to":["peer1"],"text":"first","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("first data");
    ledger::append(&ledger_path, &first).expect("first");
    let mut second = Event::new("chat.message", &group.group_id);
    second.by = "user".into();
    second.data = serde_json::json!({"to":["peer1"],"text":"second","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("second data");
    ledger::append(&ledger_path, &second).expect("second");
    let mut third = Event::new("chat.message", &group.group_id);
    third.by = "user".into();
    third.data = serde_json::json!({"to":["peer1"],"text":"third","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("third data");
    ledger::append(&ledger_path, &third).expect("third");
    assert_eq!(
        inbox::list_unread(&home, &group, "peer1", 10)
            .expect("unread")
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str(), third.id.as_str()]
    );
    let consumed =
        inbox::consume_unread(&home, &group, "peer1", "peer1", 3).expect("consume Mail prefix");
    assert_eq!(
        consumed
            .messages
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str(), third.id.as_str()]
    );
    assert!(
        inbox::list_unread(&home, &group, "peer1", 10)
            .expect("unread after advance")
            .is_empty()
    );

    let empty =
        inbox::consume_unread(&home, &group, "peer1", "peer1", 3).expect("consume empty Inbox");
    assert!(empty.messages.is_empty());
    assert!(empty.read_event.is_none());
}

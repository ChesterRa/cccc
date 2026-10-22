use cccc_contracts::{Actor, Event};
use cccc_core::{GroupStore, HomeLayout, inbox, ledger, ledger_archive};
use serde_json::json;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

fn mail(group_id: &str, number: usize) -> Event {
    let mut event = Event::new("chat.message", group_id);
    event.by = "user".into();
    event.data = json!({"to":["reader"],"message_mode":"mail","text":number.to_string()})
        .as_object()
        .expect("message data")
        .clone();
    event
}

#[test]
fn consuming_an_overflowing_inbox_preserves_order_during_append_and_rotation() {
    const EXISTING: usize = 1025;
    const APPENDED: usize = 128;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("concurrent Inbox", "").expect("group");
    group.actors.push(Actor::new("reader"));
    store.save(&group).expect("save group");
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let mut expected = Vec::new();
    for number in 0..EXISTING {
        let event = mail(&group.group_id, number);
        ledger::append(&path, &event).expect("existing Mail");
        expected.push(event.id);
    }
    assert_eq!(
        inbox::list_unread(&home, &group, "reader", 1000)
            .expect("warm overflowing page")
            .len(),
        1000
    );

    let barrier = Arc::new(Barrier::new(2));
    let writer = {
        let home = home.clone();
        let group_id = group.group_id.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let mut ids = Vec::new();
            for number in EXISTING..EXISTING + APPENDED {
                let event = mail(&group_id, number);
                ledger::append(&path, &event).expect("concurrent Mail");
                ids.push(event.id);
                if ids.len() % 32 == 0 {
                    ledger_archive::compact(&home, &group_id, "Inbox regression")
                        .expect("concurrent compaction");
                }
                std::thread::yield_now();
            }
            ids
        })
    };
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut received = Vec::new();
    while received.len() < EXISTING + APPENDED {
        assert!(Instant::now() < deadline, "Inbox stopped making progress");
        let page = inbox::consume_unread(&home, &group, "reader", "reader", 37)
            .expect("consume during writes and rotation");
        received.extend(page.messages.into_iter().map(|event| event.id));
        std::thread::yield_now();
    }
    expected.extend(writer.join().expect("writer"));
    assert_eq!(
        received, expected,
        "each Mail is consumed once, in ledger order"
    );
    assert!(
        inbox::list_unread(&home, &group, "reader", 10)
            .expect("final page")
            .is_empty()
    );
}

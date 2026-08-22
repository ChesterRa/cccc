// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupStore, HomeLayout, actors, inbox, ledger};
use serde_json::json;

#[test]
fn persisted_mail_cursor_seeds_an_empty_rust_inbox() {
    let fixture = Fixture::new();
    let old = fixture.append_message("old");
    let new = fixture.append_message("new");
    fixture.write_legacy_cursor(&old);

    let unread = fixture.unread();

    assert_eq!(
        unread
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![new.id.as_str()]
    );
}

#[test]
fn later_mail_cursor_wins_in_shared_state() {
    let fixture = Fixture::new();
    let rust_cursor = fixture.append_message("rust cursor");
    let legacy_cursor = fixture.append_message("legacy cursor");
    let latest = fixture.append_message("latest");
    let consumed = fixture.consume(1);
    assert_eq!(consumed.messages[0].id, rust_cursor.id);
    fixture.write_legacy_cursor(&legacy_cursor);

    assert_eq!(
        fixture
            .unread()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![latest.id.as_str()]
    );

    let consumed = fixture.consume(50);
    assert_eq!(consumed.messages[0].id, latest.id);
    assert!(fixture.unread().is_empty());
    assert_eq!(
        inbox::cursor(&fixture.home, &fixture.group_id, "peer").expect("cursor"),
        Some(latest.id)
    );
}

#[test]
fn pre_mail_cursor_documents_are_not_delivery_boundaries() {
    let fixture = Fixture::new();
    let first = fixture.append_message("first");
    let second = fixture.append_message("second");
    fixture.write_pre_mail_cursor(&second);

    assert_eq!(
        fixture
            .unread()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str()]
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group_id: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("legacy inbox", "").expect("group");
        let group_id = group.group_id;
        store
            .mutate(&group_id, |group| actors::add(group, Actor::new("peer")))
            .expect("actor");
        Self {
            _temp: temp,
            home,
            store,
            group_id,
        }
    }

    fn append_message(&self, text: &str) -> Event {
        let mut event = Event::new("chat.message", &self.group_id);
        event.by = "user".into();
        event.data = json!({"text": text, "to": ["peer"], "message_mode": "mail"})
            .as_object()
            .cloned()
            .expect("message data");
        ledger::append(
            &self.store.ledger_path(&self.group_id).expect("ledger"),
            &event,
        )
        .expect("append");
        event
    }

    fn write_legacy_cursor(&self, event: &Event) {
        let path = self
            .store
            .state_dir(&self.group_id)
            .expect("state")
            .join("read_cursors.json");
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "schema": 1,
                "cursors": {
                    "peer": {
                        "event_id": event.id,
                        "ts": event.ts,
                        "updated_at": event.ts,
                    }
                }
            }))
            .expect("legacy cursor"),
        )
        .expect("write legacy cursor");
    }

    fn write_pre_mail_cursor(&self, event: &Event) {
        let path = self
            .store
            .state_dir(&self.group_id)
            .expect("state")
            .join("read_cursors.json");
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "peer": {
                    "event_id": event.id,
                    "ts": event.ts,
                    "updated_at": event.ts,
                }
            }))
            .expect("pre-Mail cursor"),
        )
        .expect("write pre-Mail cursor");
    }

    fn unread(&self) -> Vec<Event> {
        let group = self.store.load(&self.group_id).expect("group");
        inbox::list_unread(&self.home, &group, "peer", 50).expect("unread")
    }

    fn consume(&self, limit: usize) -> inbox::ConsumedInbox {
        let group = self.store.load(&self.group_id).expect("group");
        inbox::consume_unread(&self.home, &group, "peer", "peer", limit).expect("consume Mail")
    }
}

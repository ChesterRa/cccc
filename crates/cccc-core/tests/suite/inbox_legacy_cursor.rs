// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupStore, HomeLayout, actors, inbox, ledger};
use serde_json::json;

#[test]
fn legacy_cursor_seeds_an_empty_rust_inbox() {
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
fn later_cursor_wins_across_legacy_and_rust_state() {
    let fixture = Fixture::new();
    let rust_cursor = fixture.append_message("rust cursor");
    let legacy_cursor = fixture.append_message("legacy cursor");
    let latest = fixture.append_message("latest");
    inbox::mark_read(&fixture.home, &fixture.group_id, "peer", &rust_cursor.id)
        .expect("seed rust cursor");
    fixture.write_legacy_cursor(&legacy_cursor);

    assert_eq!(
        fixture
            .unread()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec![latest.id.as_str()]
    );

    inbox::mark_read(&fixture.home, &fixture.group_id, "peer", &latest.id)
        .expect("advance rust cursor");
    assert!(fixture.unread().is_empty());
    assert_eq!(
        inbox::cursor(&fixture.home, &fixture.group_id, "peer").expect("cursor"),
        Some(latest.id)
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
        event.data = json!({"text": text, "to": ["peer"]})
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
                "peer": {
                    "event_id": event.id,
                    "ts": event.ts,
                    "updated_at": event.ts,
                }
            }))
            .expect("legacy cursor"),
        )
        .expect("write legacy cursor");
    }

    fn unread(&self) -> Vec<Event> {
        let group = self.store.load(&self.group_id).expect("group");
        inbox::list_unread(&self.home, &group, "peer", 50).expect("unread")
    }
}

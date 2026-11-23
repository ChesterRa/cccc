use super::*;
use cccc_contracts::Actor;
use std::path::PathBuf;

const ACCEPTED_AT: &str = "2000-01-01T00:00:00Z";
const ACTORS: [&str; 3] = ["lead", "worker", "reviewer"];

struct Fixture {
    _temporary: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: GroupDoc,
    ledger: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let home = HomeLayout::from_path(temporary.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("Reply sender reminders", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                group.state = GroupState::Active;
                for id in ACTORS {
                    actors::add(group, Actor::new(id))?;
                }
                Ok(())
            })
            .expect("actors");
        let group = store.load(&group.group_id).expect("load group");
        let ledger = store.ledger_path(&group.group_id).expect("ledger path");
        let fixture = Self {
            _temporary: temporary,
            home,
            store,
            group,
            ledger,
        };
        for actor in &fixture.group.actors {
            fixture.append("actor.add", "user", json!({"actor":actor}));
        }
        fixture
    }

    fn append(&self, kind: &str, by: &str, data: Value) -> String {
        let mut event = Event::new(kind, &self.group.group_id);
        event.by = by.into();
        event.ts = ACCEPTED_AT.into();
        event.data = data.as_object().expect("event data").clone();
        ledger::append(&self.ledger, &event).expect("append event");
        event.id
    }

    fn accepted_request(&self, sender: &str, targets: &[&str]) -> String {
        let source = self.append(
            "chat.message",
            sender,
            json!({"message_mode":"request_reply","to":targets}),
        );
        for actor in targets {
            self.append(
                "runtime.delivery",
                "system",
                json!({"actor_id":actor,"source_event_id":source,"state":"accepted"}),
            );
        }
        source
    }

    // Also compares the production projection against a fresh ordered ledger
    // replay on every call, including the partially committed state.
    fn due(&self, running: &[&str]) -> Vec<Event> {
        let running = running.iter().map(|id| (*id).to_owned()).collect();
        due_unread_notices(
            &self.home,
            &self.store,
            &self.group,
            Some(&running),
            &HashSet::new(),
            UnreadPass::Scheduled,
        )
        .expect("sender reminders")
        .expect("eligible actors")
        .1
    }

    fn save(&self, notices: &[Event]) {
        for notice in notices {
            ledger::append(&self.ledger, notice).expect("append notice");
        }
    }
}

#[test]
fn reply_sender_notice_batches_by_sender_and_retries_partial_append() {
    let fixture = Fixture::new();
    let shared = fixture.accepted_request("lead", &["worker", "reviewer"]);
    let second = fixture.accepted_request("lead", &["worker"]);
    let notices = fixture.due(&ACTORS);
    assert_eq!(notices.len(), 3);
    let sender_notice = &notices[0];
    assert!(reply_sender_notice::is_notice(sender_notice));
    assert_eq!(sender_notice.data["target_actor_id"], "lead");
    assert_eq!(sender_notice.data["im_visibility"], "internal");
    assert!(!sender_notice.data.contains_key("to"));
    assert_eq!(sender_notice.data["related_event_id"], shared);
    let context = &sender_notice.data["context"];
    assert_eq!(context["source_event_ids"], json!([shared, second]));
    assert_eq!(context["count"], 3);
    let items = reply_sender_notice::obligations(sender_notice);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].recipient_actor_id, "worker");
    assert_eq!(items[0].source_event_id, shared);
    assert_eq!(items[1].recipient_actor_id, "worker");
    assert_eq!(items[1].source_event_id, second);
    assert_eq!(items[2].recipient_actor_id, "reviewer");
    assert_eq!(items[2].source_event_id, shared);
    assert!(items.iter().all(|item| item.accepted_at == ACCEPTED_AT));

    fixture.save(&notices[..1]);
    let retried = fixture.due(&ACTORS);
    assert_eq!(retried.len(), 2);
    for (retry, original) in retried.iter().zip(&notices[1..]) {
        assert_eq!(retry.data["kind"], "reply_notice");
        assert_eq!(retry.data, original.data);
    }
    fixture.save(&retried);
    assert!(fixture.due(&ACTORS).is_empty());
}

#[test]
fn reply_sender_notice_skips_unavailable_members_and_does_not_backfill() {
    let fixture = Fixture::new();
    let open = fixture.accepted_request("lead", &["worker"]);
    let replied = fixture.accepted_request("lead", &["worker"]);
    fixture.append("chat.message", "worker", json!({"reply_to":replied}));
    let cancelled = fixture.accepted_request("lead", &["worker"]);
    fixture.append(
        "chat.reply_request.cancelled",
        "lead",
        json!({"source_event_id":cancelled}),
    );
    let human = fixture.accepted_request("user", &["worker"]);

    assert!(fixture.due(&["lead", "reviewer"]).is_empty());
    let initial = fixture.due(&["worker"]);
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].data["kind"], "reply_notice");
    assert_eq!(
        initial[0].data["context"]["source_event_ids"],
        json!([open, human])
    );
    fixture.save(&initial);
    assert!(fixture.due(&ACTORS).is_empty());
}

#[test]
fn reply_sender_notice_requires_the_current_local_registration() {
    let mut fixture = Fixture::new();
    fixture.accepted_request("lead", &["worker"]);
    fixture
        .store
        .mutate(&fixture.group.group_id, |group| {
            let lead = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "lead")
                .expect("lead");
            lead.generation = "new-lead-generation".into();
            Ok(())
        })
        .expect("change registration");
    fixture.group = fixture
        .store
        .load(&fixture.group.group_id)
        .expect("load group");

    // Saving the Group alone doesn't prove registration. Writing the new
    // actor.add also cannot transfer an old request to the replacement.
    let without_registration = fixture.due(&ACTORS);
    assert_eq!(without_registration.len(), 1);
    assert_eq!(without_registration[0].data["kind"], "reply_notice");
    fixture.append(
        "actor.add",
        "user",
        json!({"actor":actors::find(&fixture.group, "lead")}),
    );
    assert_eq!(fixture.due(&ACTORS).len(), 1);

    let local = fixture.accepted_request("lead", &["reviewer"]);
    let after_registration = fixture.due(&ACTORS);
    assert_eq!(after_registration.len(), 3);
    let sender = &after_registration[0];
    assert!(reply_sender_notice::is_notice(sender));
    assert_eq!(sender.data["context"]["source_event_ids"], json!([local]));

    fixture.append("group.create", "user", json!({"imported":true}));
    let imported = fixture.due(&ACTORS);
    assert_eq!(imported.len(), 2);
    assert!(
        imported
            .iter()
            .all(|event| event.data["kind"] == "reply_notice")
    );
}

#[test]
fn reply_sender_notice_uses_the_existing_disable_setting() {
    let mut fixture = Fixture::new();
    fixture.accepted_request("lead", &["worker"]);
    fixture
        .group
        .extra
        .insert("delivery".into(), json!({"reply_notice_after_seconds":0}));
    assert!(fixture.due(&ACTORS).is_empty());
}

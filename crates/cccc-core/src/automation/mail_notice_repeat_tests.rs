use super::*;

struct Replay {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: GroupDoc,
    mail_ids: Vec<String>,
}

impl Replay {
    fn captured(notice_age: i64, repeat: Option<i64>) -> Self {
        Self::captured_records(notice_age, repeat, true)
    }

    fn captured_records(notice_age: i64, repeat: Option<i64>, include_later: bool) -> Self {
        Self::captured_fixture(
            notice_age,
            repeat,
            include_later,
            include_str!("../../tests/fixtures/mail_notice_history.json"),
        )
    }

    fn captured_fixture(
        notice_age: i64,
        repeat: Option<i64>,
        include_later: bool,
        fixture: &str,
    ) -> Self {
        let temp = tempfile::tempdir().expect("replay directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("notice replay", "").expect("group");
        let mut events: Vec<Event> = serde_json::from_str(fixture).expect("captured history");
        let notice = events
            .iter()
            .find(|e| e.kind == "system.notify")
            .expect("captured notice replay invariant");
        let captured_at =
            DateTime::parse_from_rfc3339(&notice.ts).expect("captured notice replay invariant");
        let actor_created_at = notice.data["context"]["actor_created_at"]
            .as_str()
            .expect("captured Actor generation")
            .to_owned();
        store
            .mutate(&group.group_id, |group| {
                group.state = GroupState::Active;
                let mut actor = cccc_contracts::Actor::new("recipient");
                actor.created_at = actor_created_at;
                actors::add(group, actor)?;
                let mut delivery = json!({
                    "mail_notice_after_seconds":1,"mail_notice_idle_after_seconds":0,
                    "reply_notice_after_seconds":0
                });
                if let Some(repeat) = repeat {
                    delivery["mail_notice_repeat_seconds"] = json!(repeat);
                }
                group.extra.insert("delivery".into(), delivery);
                Ok(())
            })
            .expect("replay group");
        let anchor = Utc::now() - chrono::Duration::seconds(notice_age);
        let path = store
            .ledger_path(&group.group_id)
            .expect("captured notice replay invariant");
        let mut mail_ids = Vec::new();
        for (index, event) in events.iter_mut().enumerate() {
            // Explicit playback binding: translate the whole captured timeline,
            // preserving its relative times and order. No captured fact is invented.
            let at =
                DateTime::parse_from_rfc3339(&event.ts).expect("captured notice replay invariant");
            event.ts = (anchor + (at - captured_at)).to_rfc3339();
            event.group_id = group.group_id.clone();
            if !include_later && index == 4 {
                continue;
            }
            if event.kind == "chat.message" {
                mail_ids.push(event.id.clone());
            }
            ledger::append(&path, event).expect("replay captured event");
        }
        if let Some(read) = events.iter().find(|event| event.kind == "mail.read") {
            // Restore the captured read boundary using the actual cursor
            // transaction; appending a ledger fact alone is not a cursor write.
            let group = store.load(&group.group_id).expect("replay group");
            let consumed = inbox::consume_unread(&home, &group, "recipient", "recipient", 10)
                .expect("replay captured read transaction");
            assert_eq!(
                consumed.cursor_event_id,
                read.data["event_id"]
                    .as_str()
                    .expect("captured read boundary")
            );
        }
        Self {
            _temp: temp,
            home,
            group: store
                .load(&group.group_id)
                .expect("captured notice replay invariant"),
            store,
            mail_ids,
        }
    }

    fn set_repeat(&mut self, repeat: i64) {
        self.group
            .extra
            .get_mut("delivery")
            .expect("delivery settings")["mail_notice_repeat_seconds"] = json!(repeat);
    }

    fn issue_accepted_expired(&self, mut notice: Event) {
        // These are explicit replay actions, not additional captured history.
        // Advance a successful handoff beyond the interval without sleeping.
        notice.ts = (Utc::now() - chrono::Duration::seconds(2)).to_rfc3339();
        let path = self
            .store
            .ledger_path(&self.group.group_id)
            .expect("ledger");
        ledger::append(&path, &notice).expect("issue replayed output");
        let created = &self.group.actors[0].created_at;
        let delivery_id = format!(
            "fixture:{}:recipient:{created}:{}",
            self.group.group_id, notice.id
        );
        for state in ["claimed", "accepted"] {
            let mut delivery = Event::new("runtime.delivery", &self.group.group_id);
            delivery.by = "system".into();
            delivery.ts = notice.ts.clone();
            delivery.data = json!({
                "actor_id":"recipient", "actor_created_at":created,
                "source_event_id":notice.id, "delivery_id":delivery_id,
                "transport":"pty", "state":state
            })
            .as_object()
            .expect("controlled handoff")
            .clone();
            ledger::append(&path, &delivery).expect("replay handoff");
        }
    }

    fn due(&self) -> Vec<Event> {
        // This is the production projection, plus the cfg(test)-only ordered
        // reference reducer assertion in due_unread_notices.
        let recipients = HashSet::from(["recipient".to_owned()]);
        due_unread_notices(
            &self.home,
            &self.store,
            &self.group,
            Some(&recipients),
            &HashSet::new(),
            UnreadPass::Scheduled,
        )
        .expect("projection / reference equivalence")
        .expect("eligible recipient")
        .1
    }
}

#[test]
fn mail_notice_repeat_carries_both_captured_pending_mail() {
    let replay = Replay::captured(3610, Some(3600));
    let due = replay.due();
    assert_eq!(
        due.len(),
        1,
        "accepted notice must not hold unread Mail forever"
    );
    assert_eq!(
        due[0].data["context"]["source_event_ids"],
        json!(replay.mail_ids)
    );
    assert_eq!(due[0].data["context"]["count"], 2);
    assert_eq!(
        inbox::list_unread(&replay.home, &replay.group, "recipient", 10)
            .expect("captured notice replay invariant")
            .len(),
        2,
        "a notice does not consume or promote its Mail"
    );
    ledger::append(
        &replay
            .store
            .ledger_path(&replay.group.group_id)
            .expect("captured notice replay invariant"),
        &due[0],
    )
    .expect("issue repeat");
    assert!(replay.due().is_empty(), "the repeat starts a new interval");
}

#[test]
fn mail_notice_repeat_does_not_fire_before_its_interval() {
    let replay = Replay::captured(3590, Some(3600));
    assert_eq!(replay.mail_ids.len(), 2);
    assert!(replay.due().is_empty());
}

#[test]
fn mail_notice_repeat_default_and_zero_keep_one_shot_behavior() {
    for setting in [None, Some(0)] {
        let replay = Replay::captured(3610, setting);
        assert!(replay.due().is_empty(), "setting {setting:?}");
    }
}

#[test]
fn mail_notice_repeat_warm_projection_does_not_replay_history() {
    let replay = Replay::captured(3610, Some(3600));
    assert_eq!(replay.due().len(), 1);
    let decoded = crate::ledger::snapshot::decoded_records();
    assert!(decoded > 0, "cold projection must decode captured records");
    for _ in 0..5 {
        assert_eq!(replay.due().len(), 1);
    }
    assert_eq!(crate::ledger::snapshot::decoded_records(), decoded);
}

#[test]
fn mail_notice_repeat_continues_after_multiple_successful_handoffs() {
    let replay = Replay::captured_records(10, Some(1), false);
    for _ in 0..8 {
        let due = replay.due();
        assert_eq!(due.len(), 1, "unread Mail remains notice-eligible");
        replay.issue_accepted_expired(due.into_iter().next().expect("repeat"));
    }
    assert_eq!(replay.due().len(), 1);

    let later = Replay::captured(3610, Some(3600));
    let due = later.due();
    assert_eq!(due.len(), 1, "a changed batch has an initial notice");
    assert_eq!(due[0].data["context"]["count"], 2);
}

#[test]
fn mail_notice_repeat_prunes_claims_across_many_intervals_and_rebuilds() {
    let mut replay = Replay::captured_records(10, Some(1), false);
    for _ in 0..64 {
        let due = replay.due();
        assert_eq!(due.len(), 1);
        assert!(
            inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
                .expect("claim count")
                <= 1
        );
        replay.issue_accepted_expired(due.into_iter().next().expect("repeat"));
    }
    assert_eq!(replay.due().len(), 1);
    assert!(
        inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
            .expect("claim count")
            <= 1
    );
    // Disabling repeats must reconstruct one-shot ownership after pruning.
    replay.set_repeat(0);
    assert!(
        replay.due().is_empty(),
        "historical handoffs still hold the batch"
    );
    replay.set_repeat(1);
    assert_eq!(
        replay.due().len(),
        1,
        "re-enabling restores repeat eligibility"
    );
    assert!(
        inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
            .expect("claim count")
            <= 1
    );
}

#[test]
fn mail_notice_repeat_captured_later_boundary_read_closes_earlier_mail() {
    assert_eq!(
        Replay::captured(3610, Some(3600)).due().len(),
        1,
        "unread control"
    );
    let replay = Replay::captured_fixture(
        3610,
        Some(3600),
        true,
        include_str!("../../tests/fixtures/mail_notice_read_history.json"),
    );
    assert!(
        replay.due().is_empty(),
        "the captured later boundary closes both Mail before repeat"
    );
    assert!(
        inbox::list_unread(&replay.home, &replay.group, "recipient", 10)
            .expect("read cursor")
            .is_empty()
    );
}

#[test]
fn mail_notice_repeat_generation_change_refreshes_the_cached_group() {
    let mut replay = Replay::captured_records(10, Some(1), false);
    assert_eq!(replay.due().len(), 1, "warm the old generation");
    let old_generation = replay.group.actors[0].created_at.clone();

    // Exercise the core projection boundary without intervening daemon response
    // queries that can rebuild its cache while the Actor list is empty.
    actors::remove(&mut replay.group, "recipient").expect("remove Actor");
    let actor = actors::add(&mut replay.group, cccc_contracts::Actor::new("recipient"))
        .expect("new Actor generation");
    // Inspect the exact cached Group consumed by append-time pruning before
    // any ledger mutation can independently trigger a snapshot rebuild.
    assert_ne!(actor.created_at, old_generation);
    assert_eq!(
        inbox::projection::cached_actor_generation(&replay.home, &replay.group, "recipient")
            .expect("current projection metadata"),
        Some(actor.created_at.clone()),
        "same-ID replacement must refresh the cached generation before pruning"
    );
    let mut added = Event::new("actor.add", &replay.group.group_id);
    added.data = json!({"actor":actor})
        .as_object()
        .expect("Actor event")
        .clone();
    let path = replay
        .store
        .ledger_path(&replay.group.group_id)
        .expect("ledger");
    ledger::append(&path, &added).expect("record actual core Actor operation");
    let fixture: Vec<Event> = serde_json::from_str(include_str!(
        "../../tests/fixtures/mail_notice_history.json"
    ))
    .expect("captured Mail payload");
    let mut mail = Event::new("chat.message", &replay.group.group_id);
    mail.by = "sender".into();
    mail.data = fixture[0].data.clone();
    ledger::append(&path, &mail).expect("replay a new-generation Mail operation");
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let initial = replay.due();
    assert_eq!(initial.len(), 1, "the new batch has an initial notice");
    assert_eq!(
        initial[0].data["context"]["source_event_ids"],
        json!([mail.id])
    );
    replay.issue_accepted_expired(initial.into_iter().next().expect("new initial notice"));
    let repeated = replay.due();
    assert_eq!(
        repeated.len(),
        1,
        "the new generation can repeat after handoff"
    );
    assert_eq!(
        repeated[0].data["context"]["source_event_ids"],
        json!([mail.id])
    );
}

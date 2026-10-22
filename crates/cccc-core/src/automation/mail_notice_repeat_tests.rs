use super::*;

struct Replay {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: GroupDoc,
    mail_ids: Vec<String>,
    later_mail: Option<Event>,
}

impl Replay {
    fn captured(notice_age: i64, repeat: Option<i64>) -> Self {
        Self::captured_records(notice_age, repeat, true)
    }

    fn captured_records(notice_age: i64, repeat: Option<i64>, include_later: bool) -> Self {
        let temp = tempfile::tempdir().expect("replay directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("notice replay", "").expect("group");
        let mut events: Vec<Event> = serde_json::from_str(include_str!(
            "../../tests/fixtures/mail_notice_history.json"
        ))
        .expect("captured history");
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
        let mut later_mail = None;
        for (index, event) in events.iter_mut().enumerate() {
            // Explicit playback binding: translate the whole captured timeline,
            // preserving its relative times and order. No captured fact is invented.
            let at =
                DateTime::parse_from_rfc3339(&event.ts).expect("captured notice replay invariant");
            event.ts = (anchor + (at - captured_at)).to_rfc3339();
            event.group_id = group.group_id.clone();
            if !include_later && index == 4 {
                later_mail = Some(event.clone());
                continue;
            }
            if event.kind == "chat.message" {
                mail_ids.push(event.id.clone());
            }
            ledger::append(&path, event).expect("replay captured event");
        }
        Self {
            _temp: temp,
            home,
            group: store
                .load(&group.group_id)
                .expect("captured notice replay invariant"),
            store,
            mail_ids,
            later_mail,
        }
    }

    fn set_max(&mut self, max: i64) {
        self.group
            .extra
            .get_mut("delivery")
            .expect("delivery settings")["mail_notice_repeat_max"] = json!(max);
    }

    fn issue_expired(&self, mut notice: Event) {
        // Replay the production-issued output at the fixture's translated
        // issue time, advancing one interval without a wall-clock sleep.
        notice.ts = (Utc::now() - chrono::Duration::seconds(3610)).to_rfc3339();
        ledger::append(
            &self
                .store
                .ledger_path(&self.group.group_id)
                .expect("ledger"),
            &notice,
        )
        .expect("issue replayed output");
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
        let replay = Replay::captured(86400, setting);
        assert!(replay.due().is_empty(), "setting {setting:?}");
    }
}

#[test]
fn mail_notice_repeat_warm_projection_does_not_replay_history() {
    let replay = Replay::captured(3610, Some(3600));
    assert_eq!(replay.due().len(), 1);
    let decoded = crate::ledger::snapshot::decoded_records();
    for _ in 0..5 {
        assert_eq!(replay.due().len(), 1);
    }
    assert_eq!(crate::ledger::snapshot::decoded_records(), decoded);
}

#[test]
fn mail_notice_repeat_stops_at_exact_batch_max_and_resets_for_later_mail() {
    for max in [0, 1, 3] {
        // A real prefix ending at accepted delivery, before the captured later Mail.
        let mut replay = Replay::captured_records(3610, Some(3600), false);
        replay.set_max(max);
        for _ in 0..max {
            let due = replay.due();
            assert_eq!(due.len(), 1, "repeat budget {max}");
            replay.issue_expired(due.into_iter().next().expect("repeat"));
        }
        assert!(
            replay.due().is_empty(),
            "exactly {max} repeats, excluding the captured initial notice"
        );
        assert!(
            replay.due().is_empty(),
            "pruning must not reset the counter"
        );
        ledger::append(
            &replay
                .store
                .ledger_path(&replay.group.group_id)
                .expect("ledger"),
            replay.later_mail.as_ref().expect("captured later Mail"),
        )
        .expect("append real later Mail");
        let due = replay.due();
        assert_eq!(due.len(), 1, "a new pending batch has an initial notice");
        assert_eq!(due[0].data["context"]["count"], 2);
    }
    let replay = Replay::captured_records(3610, Some(3600), false);
    for _ in 0..3 {
        let due = replay.due();
        assert_eq!(due.len(), 1, "default max is three");
        replay.issue_expired(due.into_iter().next().expect("repeat"));
    }
    assert!(replay.due().is_empty());
}

#[test]
fn mail_notice_repeat_prunes_claims_across_many_intervals_and_rebuilds() {
    let mut replay = Replay::captured_records(3610, Some(3600), false);
    replay.set_max(64);
    for _ in 0..64 {
        let due = replay.due();
        assert_eq!(due.len(), 1);
        assert!(
            inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
                .expect("claim count")
                <= 1
        );
        replay.issue_expired(due.into_iter().next().expect("repeat"));
    }
    assert!(replay.due().is_empty());
    assert!(
        inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
            .expect("claim count")
            <= 1
    );
    // A setting change rebuilds the production projection from real issued
    // records; neither the finite budget nor pruned claim state can reset.
    replay.set_max(63);
    assert!(replay.due().is_empty(), "rebuild preserves exhaustion");
    assert!(
        inbox::projection::retained_mail_notice_claims(&replay.home, &replay.group)
            .expect("claim count")
            <= 1
    );
}

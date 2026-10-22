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
        for event in &mut events {
            // Explicit playback binding: translate the whole captured timeline,
            // preserving its relative times and order. No captured fact is invented.
            let at =
                DateTime::parse_from_rfc3339(&event.ts).expect("captured notice replay invariant");
            event.ts = (anchor + (at - captured_at)).to_rfc3339();
            event.group_id = group.group_id.clone();
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
        let replay = Replay::captured(86400, setting);
        assert!(replay.due().is_empty(), "setting {setting:?}");
    }
}

#[test]
#[ignore = "run alone: process-wide projection decode counter"]
fn mail_notice_repeat_warm_projection_does_not_replay_history() {
    let replay = Replay::captured(3610, Some(3600));
    assert_eq!(replay.due().len(), 1);
    let decoded = crate::ledger::snapshot::decoded_records();
    for _ in 0..5 {
        assert_eq!(replay.due().len(), 1);
    }
    assert_eq!(crate::ledger::snapshot::decoded_records(), decoded);
}

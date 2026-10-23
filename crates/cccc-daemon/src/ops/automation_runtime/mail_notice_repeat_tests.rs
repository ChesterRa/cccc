use super::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::{Duration, Instant};

struct Replay {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
    mail_ids: Vec<String>,
}

impl Replay {
    fn captured(notice_age: i64, repeat: Option<i64>) -> Self {
        Self::captured_fixture(notice_age, repeat, false)
    }

    fn captured_fixture(notice_age: i64, repeat: Option<i64>, read_stop: bool) -> Self {
        let temp = tempfile::tempdir().expect("private replay");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let response = request(&home, "group_create", json!({"title":"notice replay"}));
        let group_id = response["group"]["group_id"]
            .as_str()
            .expect("captured notice replay invariant")
            .to_owned();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).expect("captured notice replay invariant");
        request(
            &home,
            "attach",
            json!({"group_id":group_id,"path":project,"by":"user"}),
        );
        add_actor(&home, &group_id);
        let mut patch = json!({"mail_notice_after_seconds":1,"mail_notice_idle_after_seconds":0,"reply_notice_after_seconds":0});
        if let Some(repeat) = repeat {
            patch["mail_notice_repeat_seconds"] = json!(repeat);
        }
        request(
            &home,
            "group_settings_update",
            json!({"group_id":group_id,"by":"user","patch":patch}),
        );
        let store = GroupStore::new(home.clone()).expect("captured notice replay invariant");
        store
            .mutate(&group_id, |g| {
                g.state = cccc_contracts::GroupState::Active;
                Ok(())
            })
            .expect("captured notice replay invariant");
        let group = store
            .load(&group_id)
            .expect("captured notice replay invariant");
        let actor_created_at = group
            .actors
            .iter()
            .find(|a| a.id == "recipient")
            .expect("captured notice replay invariant")
            .created_at
            .clone();
        let fixture = if read_stop {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../cccc-core/tests/fixtures/mail_notice_read_history.json"
            ))
        } else {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../cccc-core/tests/fixtures/mail_notice_history.json"
            ))
        };
        let mut events: Vec<Event> =
            serde_json::from_str(fixture).expect("captured notice replay invariant");
        let captured_at = DateTime::parse_from_rfc3339(
            &events
                .iter()
                .find(|e| e.kind == "system.notify")
                .expect("captured notice replay invariant")
                .ts,
        )
        .expect("captured notice replay invariant");
        let anchor = Utc::now() - chrono::Duration::seconds(notice_age);
        let path = store
            .ledger_path(&group_id)
            .expect("captured notice replay invariant");
        let mut mail_ids = Vec::new();
        for event in &mut events {
            let at =
                DateTime::parse_from_rfc3339(&event.ts).expect("captured notice replay invariant");
            event.ts = (anchor + (at - captured_at)).to_rfc3339();
            event.group_id = group_id.clone();
            if event.kind == "chat.message" {
                mail_ids.push(event.id.clone());
            }
            if event.kind == "system.notify" {
                event
                    .data
                    .get_mut("context")
                    .expect("captured notice replay invariant")["actor_created_at"] =
                    json!(actor_created_at);
            }
            cccc_core::ledger::append(&path, event).expect("captured notice replay invariant");
        }
        if let Some(read) = events.iter().find(|event| event.kind == "mail.read") {
            // Replay the cursor half of the captured read via the actual daemon.
            let consumed = request(
                &home,
                "inbox_read",
                json!({"group_id":group_id,"actor_id":"recipient","by":"recipient"}),
            );
            assert_eq!(
                consumed["messages"]
                    .as_array()
                    .expect("captured read messages")
                    .last()
                    .expect("captured read boundary")["id"],
                read.data["event_id"]
            );
        }
        // The real actor.start still begins its normal one-second Mail window.
        std::thread::sleep(Duration::from_millis(1200));
        Self {
            _temp: temp,
            home,
            group_id,
            mail_ids,
        }
    }

    fn notices(&self) -> Vec<Event> {
        let store = GroupStore::new(self.home.clone()).expect("captured notice replay invariant");
        cccc_core::ledger::read_all(
            &store
                .ledger_path(&self.group_id)
                .expect("captured notice replay invariant"),
        )
        .expect("captured notice replay invariant")
        .into_iter()
        .filter(|e| e.kind == "system.notify" && e.data["kind"] == "mail_notice")
        .collect()
    }

    fn tick(&self) {
        // The same eligibility, projection, notice append and dispatch path
        // used by the daemon's scheduled unread pass; not the reference reducer.
        tick_group(&self.home, &self.group_id, true, &AtomicBool::new(false));
    }

    fn accepted(&self, event_id: &str) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let state = crate::ops::runtime_delivery::latest_state(
                &self.home,
                &self.group_id,
                "recipient",
                event_id,
            )
            .expect("captured notice replay invariant");
            if state.as_ref().is_some_and(|s| s.0 == "accepted") {
                return;
            }
            assert!(
                Instant::now() < until,
                "real delivery was not accepted: {state:?}"
            );
            actor_delivery::drain_group(&self.home, &self.group_id);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn repeat(&self) -> Event {
        assert_eq!(self.notices().len(), 1, "one captured accepted notice");
        self.tick();
        let notices = self.notices();
        assert_eq!(
            notices.len(),
            2,
            "production daemon must repeat an accepted-but-unread notice"
        );
        let repeated = notices
            .last()
            .expect("captured notice replay invariant")
            .clone();
        assert_eq!(
            repeated.data["context"]["source_event_ids"],
            json!(self.mail_ids)
        );
        self.accepted(&repeated.id);
        repeated
    }

    fn expect_no_repeat(&self) {
        let before = self.notices().len();
        self.tick();
        assert_eq!(self.notices().len(), before);
    }

    fn resolve(&self, resolution: &str) {
        match resolution {
            "read" => {
                let result = request(
                    &self.home,
                    "inbox_read",
                    json!({"group_id":self.group_id,"actor_id":"recipient","by":"recipient"}),
                );
                assert_eq!(
                    result["messages"]
                        .as_array()
                        .expect("captured notice replay invariant")
                        .len(),
                    2
                );
            }
            "reply" => {
                for id in &self.mail_ids {
                    request(
                        &self.home,
                        "reply",
                        json!({"group_id":self.group_id,"by":"recipient","to":["user"],"reply_to":id,"text":"Acknowledged","message_mode":"send"}),
                    );
                }
            }
            "delivery" => {
                for id in &self.mail_ids {
                    request(
                        &self.home,
                        "message_deliver",
                        json!({"group_id":self.group_id,"by":"user","source_event_id":id,"actor_ids":["recipient"]}),
                    );
                    self.accepted(id);
                }
            }
            "generation" => {
                request(
                    &self.home,
                    "actor_remove",
                    json!({"group_id":self.group_id,"actor_id":"recipient","by":"user"}),
                );
                add_actor(&self.home, &self.group_id);
                std::thread::sleep(Duration::from_millis(1200));
            }
            _ => panic!("unknown resolution"),
        }
    }
}

impl Drop for Replay {
    fn drop(&mut self) {
        let response = crate::dispatch::dispatch(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "actor_stop".into(),
                args: json!({"group_id":self.group_id,"actor_id":"recipient","by":"user"})
                    .as_object()
                    .expect("captured notice replay invariant")
                    .clone(),
            },
        );
        if !std::thread::panicking() {
            assert!(response.ok, "private Actor cleanup: {response:?}");
        }
    }
}

fn request(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = crate::dispatch::dispatch(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args
                .as_object()
                .expect("captured notice replay invariant")
                .clone(),
        },
    );
    assert!(response.ok, "{op}: {response:?}");
    Value::Object(response.result)
}

fn add_actor(home: &HomeLayout, group: &str) {
    request(
        home,
        "actor_add",
        json!({"group_id":group,"actor_id":"recipient","runtime":"custom","command":["/bin/cat"],"submit":"none","by":"user"}),
    );
    request(
        home,
        "actor_start",
        json!({"group_id":group,"actor_id":"recipient","by":"user"}),
    );
}

#[test]
fn mail_notice_repeat_daemon_accepts_repeat_with_later_mail() {
    let replay = Replay::captured(3610, Some(3600));
    let repeated = replay.repeat();
    assert_eq!(repeated.data["context"]["count"], 2);
    replay.expect_no_repeat();
    let inbox = request(
        &replay.home,
        "inbox_peek",
        json!({"group_id":replay.group_id,"actor_id":"recipient","by":"recipient"}),
    );
    assert_eq!(
        inbox["messages"]
            .as_array()
            .expect("captured notice replay invariant")
            .len(),
        2
    );
}

#[test]
fn mail_notice_repeat_daemon_preserves_interval_and_default() {
    for (age, repeat) in [(3590, Some(3600)), (3610, None), (3610, Some(0))] {
        let replay = Replay::captured(age, repeat);
        replay.expect_no_repeat();
    }
}

fn closes_repeat(resolution: &str) {
    let replay = Replay::captured(3610, Some(3600));
    replay.repeat(); // Positive control: this exact recipient and history can repeat.
    replay.resolve(resolution);
    // Expire the new notice too, so the assertion cannot pass just because
    // that notice's interval still holds its batching claim.
    request(
        &replay.home,
        "group_settings_update",
        json!({"group_id":replay.group_id,"by":"user","patch":{"mail_notice_repeat_seconds":1}}),
    );
    std::thread::sleep(Duration::from_millis(1200));
    replay.expect_no_repeat();
    if resolution == "generation" {
        let sent = request(
            &replay.home,
            "send",
            json!({"group_id":replay.group_id,"by":"user","to":["recipient"],"text":"New generation Mail","message_mode":"mail"}),
        );
        std::thread::sleep(Duration::from_millis(1200));
        let before = replay.notices().len();
        replay.tick();
        let notices = replay.notices();
        assert_eq!(
            notices.len(),
            before + 1,
            "new generation is still notice-eligible"
        );
        assert_eq!(
            notices
                .last()
                .expect("captured notice replay invariant")
                .data["context"]["source_event_ids"],
            json!([sent["event"]["id"]])
        );
    }
}

#[test]
fn mail_notice_repeat_daemon_stops_after_read() {
    closes_repeat("read");
}
#[test]
fn mail_notice_repeat_daemon_stops_after_reply() {
    closes_repeat("reply");
}
#[test]
fn mail_notice_repeat_daemon_stops_after_mail_delivery() {
    closes_repeat("delivery");
}
#[test]
fn mail_notice_repeat_daemon_stops_after_actor_generation_change() {
    closes_repeat("generation");
}

#[test]
fn mail_notice_repeat_daemon_continues_while_the_same_mail_is_unread() {
    for replaced in [false, true] {
        let replay = Replay::captured(3610, Some(1));
        if replaced {
            // Keep a warm cache across removal/re-addition of the same Actor ID.
            replay.repeat();
            replay.resolve("generation");
            request(
                &replay.home,
                "send",
                json!({"group_id":replay.group_id,"by":"user","to":["recipient"],"text":"New generation Mail","message_mode":"mail"}),
            );
            std::thread::sleep(Duration::from_millis(1200));
            let before = replay.notices().len();
            replay.tick();
            let notices = replay.notices();
            assert_eq!(notices.len(), before + 1);
            replay.accepted(&notices.last().expect("new generation initial").id);
        } else {
            replay.repeat(); // Initial notice for the two-Mail pending batch.
        }
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(1200));
            let before = replay.notices().len();
            replay.tick();
            let notices = replay.notices();
            assert_eq!(notices.len(), before + 1);
            replay.accepted(&notices.last().expect("repeat while unread").id);
        }
    }
}

#[test]
fn mail_notice_repeat_daemon_continues_after_restart_and_new_session() {
    for op in ["actor_restart", "actor_new_session"] {
        let replay = Replay::captured(3610, Some(3600));
        replay.repeat();
        let store = GroupStore::new(replay.home.clone()).expect("group store");
        let created = store.load(&replay.group_id).expect("group").actors[0]
            .created_at
            .clone();
        request(
            &replay.home,
            "group_settings_update",
            json!({"group_id":replay.group_id,"by":"user","patch":{"mail_notice_repeat_seconds":1}}),
        );
        // Bind the new settings to a warm projection before the lifecycle op,
        // without issuing a notice while its one-second claim may expire.
        request(
            &replay.home,
            "inbox_peek",
            json!({"group_id":replay.group_id,"actor_id":"recipient","by":"recipient"}),
        );
        request(
            &replay.home,
            op,
            json!({"group_id":replay.group_id,"actor_id":"recipient","by":"user"}),
        );
        assert_eq!(
            store.load(&replay.group_id).expect("group").actors[0].created_at,
            created,
            "runtime lifecycle is not a new Actor generation"
        );

        std::thread::sleep(Duration::from_millis(1200));
        let before = replay.notices().len();
        replay.tick();
        let notices = replay.notices();
        assert_eq!(
            notices.len(),
            before + 1,
            "{op} preserves pending Mail and repeat eligibility"
        );
        let notice = notices.last().expect("repeat after runtime lifecycle");
        assert_eq!(
            notice.data["context"]["source_event_ids"],
            json!(replay.mail_ids)
        );
        replay.accepted(&notice.id);
        std::thread::sleep(Duration::from_millis(1200));
        let before = replay.notices().len();
        replay.tick();
        let repeated = replay.notices();
        assert_eq!(
            repeated.len(),
            before + 1,
            "unread Mail remains eligible after {op}"
        );
        replay.accepted(&repeated.last().expect("subsequent repeat").id);
    }
}

#[test]
fn mail_notice_repeat_daemon_captured_later_boundary_read_stops_before_repeat() {
    let replay = Replay::captured_fixture(3610, Some(3600), true);
    replay.expect_no_repeat();
    let inbox = request(
        &replay.home,
        "inbox_peek",
        json!({"group_id":replay.group_id,"actor_id":"recipient","by":"recipient"}),
    );
    assert!(
        inbox["messages"]
            .as_array()
            .expect("captured read Inbox")
            .is_empty()
    );
}

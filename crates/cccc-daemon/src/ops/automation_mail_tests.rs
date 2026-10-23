//! Offline Mail-notice delivery and recovery boundaries; no provider CLI is used.
use super::*;
use crate::ops::runtime_delivery::{self, ClaimResult, DeliveryOutcome};
use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, GroupState, RunnerKind};
use cccc_core::ledger;
use serde_json::Value;
use std::time::{Duration, Instant};

struct Fixture {
    temp: tempfile::TempDir,
    home: HomeLayout,
    store: GroupStore,
    group: GroupDoc,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("repeat Mail", "").expect("group");
        let mut group = cccc_core::group_scope::attach(
            &store,
            &group.group_id,
            cccc_core::scope::detect(temp.path()).expect("scope"),
        )
        .expect("attach");
        group.state = GroupState::Active;
        let mut actor = Actor::new("peer");
        actor.runtime = ActorRuntime::Custom;
        actor.submit = ActorSubmit::Newline;
        actors::add(&mut group, actor).expect("actor");
        group.extra.insert(
            "delivery".into(),
            json!({"mail_notice_after_seconds":1,"mail_notice_idle_after_seconds":0}),
        );
        store.save(&group).expect("save");
        Self {
            temp,
            home,
            store,
            group,
        }
    }

    fn append(&self, kind: &str, by: &str, data: Value, age: i64) -> Event {
        let mut event = Event::new(kind, &self.group.group_id);
        event.by = by.into();
        event.ts = (chrono::Utc::now() - chrono::Duration::seconds(age)).to_rfc3339();
        event.data = data.as_object().expect("event data").clone();
        ledger::append(
            &self
                .store
                .ledger_path(&self.group.group_id)
                .expect("ledger"),
            &event,
        )
        .expect("append");
        event
    }

    fn mail(&self) -> Event {
        self.append(
            "chat.message",
            "user",
            json!({"to":["peer"],"text":"MAIL_BODY_MUST_STAY_IN_INBOX","message_mode":"mail"}),
            600,
        )
    }

    fn configure(&self, repeat: i64) {
        let response = crate::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "group_settings_update".into(),
                args: json!({"group_id":self.group.group_id,"by":"user","patch":{"mail_notice_repeat_seconds":repeat}})
                    .as_object().expect("settings").clone(),
            },
        );
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(
            response.result["settings"]["mail_notice_repeat_seconds"],
            repeat
        );
    }

    fn notice(&self, mail: &Event, state: Option<&str>) -> Event {
        let actor = &self.group.actors[0];
        let notice = self.append(
            "system.notify",
            "system",
            json!({"kind":"mail_notice","target_actor_id":"peer","title":"Mail waiting",
                "message":"Call cccc_inbox_read when appropriate.","im_visibility":"internal",
                "context":{"actor_id":"peer","actor_created_at":actor.created_at,
                    "source_event_ids":[mail.id],"count":1}}),
            120,
        );
        if let Some(state) = state {
            assert_eq!(
                runtime_delivery::claim(&self.home, &self.group, actor, &notice.id, "pty", false)
                    .expect("claim"),
                ClaimResult::Claimed
            );
            let outcome = match state {
                "accepted" => Some(DeliveryOutcome::Accepted),
                "failed" => Some(DeliveryOutcome::Failed("fixture handoff failed")),
                "claimed" => None,
                _ => panic!("unknown fixture delivery state"),
            };
            if let Some(outcome) = outcome {
                runtime_delivery::append_state(
                    &self.home,
                    &self.group.group_id,
                    "peer",
                    &actor.created_at,
                    &notice.id,
                    "pty",
                    outcome,
                )
                .expect("accepted");
            }
        }
        notice
    }

    fn tick(&self) -> Vec<Event> {
        cccc_core::automation::tick_group_for_delivery_actors(
            &self.home,
            &self.group.group_id,
            true,
            &HashSet::from(["peer".to_owned()]),
        )
        .expect("tick")
        .notifications
    }

    fn notices(&self) -> Vec<Event> {
        ledger::read_all(
            &self
                .store
                .ledger_path(&self.group.group_id)
                .expect("ledger"),
        )
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "system.notify" && event.data["kind"] == "mail_notice")
        .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        actor_delivery::shutdown_group(&self.group.group_id);
        let _ = cccc_runtime::stop(&self.group.group_id, "peer");
    }
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready() {
        assert!(Instant::now() < deadline, "fixture timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn repeat_mail_reaches_the_terminal_without_pushing_bodies_and_stops_after_read() {
    let f = Fixture::new();
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: f.group.group_id.clone(),
        actor_id: "peer".into(),
        runner: RunnerKind::Pty,
        command: vec![
            "python3".into(),
            "-u".into(),
            "-c".into(),
            r#"
import json,os,pathlib,sys,tty
root=pathlib.Path(sys.argv[1]); tty.setraw(0); (root/'ready').touch(); buf=b''
while True:
 data=os.read(0,65536)
 if not data: break
 buf+=data
 while b'\n' in buf:
  text,buf=buf.split(b'\n',1)
  with (root/'received').open('a') as out: out.write(json.dumps(text.decode())+'\n')
"#
            .into(),
            f.temp.path().to_string_lossy().into_owned(),
        ],
        cwd: f.temp.path().to_path_buf(),
        env: BTreeMap::from([(
            "CODEX_HOME".into(),
            f.temp.path().join("codex").to_string_lossy().into_owned(),
        )]),
        cols: 120,
        rows: 40,
    })
    .expect("terminal fixture");
    wait_for(|| f.temp.path().join("ready").exists());
    let first_mail = f.mail();
    let cancelled = AtomicBool::new(false);
    tick_group(&f.home, &f.group.group_id, true, &cancelled);
    let first = f.notices().pop().expect("first notice");
    let accepted = |notice: &Event| {
        runtime_delivery::latest_state(&f.home, &f.group.group_id, "peer", &notice.id)
            .expect("state")
            .is_some_and(|state| state.0 == "accepted")
    };
    wait_for(|| accepted(&first));
    let later_mail = f.mail();
    tick_group(&f.home, &f.group.group_id, true, &cancelled);
    assert_eq!(f.notices().len(), 1, "the default remains one-shot");

    f.configure(1);
    tick_group(&f.home, &f.group.group_id, true, &cancelled);
    let notices = f.notices();
    assert_eq!(notices.len(), 2);
    let repeated = &notices[1];
    assert_ne!(first.id, repeated.id);
    assert_eq!(
        repeated.data["context"]["source_event_ids"],
        json!([first_mail.id, later_mail.id])
    );
    wait_for(|| accepted(repeated));

    let read = crate::handle_request(
        &f.home,
        &DaemonRequest {
            v: 1,
            op: "inbox_read".into(),
            args: json!({"group_id":f.group.group_id,"actor_id":"peer","by":"peer","limit":50})
                .as_object()
                .expect("read args")
                .clone(),
        },
    );
    assert!(read.ok, "{:?}", read.error);
    assert_eq!(read.result["messages"].as_array().expect("Mail").len(), 2);
    tick_group(&f.home, &f.group.group_id, true, &cancelled);
    assert_eq!(f.notices().len(), 2);
    wait_for(|| {
        std::fs::read_to_string(f.temp.path().join("received"))
            .is_ok_and(|text| text.matches("Call cccc_inbox_read").count() == 2)
    });
    let received = std::fs::read_to_string(f.temp.path().join("received")).expect("received");
    assert!(!received.contains("MAIL_BODY_MUST_STAY_IN_INBOX"));
}

#[test]
fn pending_mail_notices_do_not_accumulate_repeats_and_restart_settles_the_claim() {
    for state in [None, Some("claimed"), Some("failed")] {
        let f = Fixture::new();
        f.configure(60);
        let mail = f.mail();
        let first = f.notice(&mail, state);
        assert!(
            f.tick().is_empty(),
            "a pending {state:?} notice still owns the handoff"
        );
        if state == Some("claimed") {
            assert_eq!(
                runtime_delivery::settle_stranded_claims(&f.home, &f.group).expect("restart"),
                1
            );
            let repeated = f.tick();
            assert_eq!(repeated.len(), 1);
            assert_ne!(repeated[0].id, first.id);
            assert_eq!(
                repeated[0].data["context"]["source_event_ids"],
                json!([mail.id])
            );
            assert!(f.tick().is_empty(), "the new notice has its own handoff");
        }
    }
}

#[test]
fn withdrawn_mail_notices_remain_eligible_until_a_handoff_succeeds() {
    for repeat in [0, 3600] {
        let f = Fixture::new();
        f.configure(repeat);
        let mail = f.mail();
        for attempt in 0..8 {
            let notices = f.tick();
            assert_eq!(
                notices.len(),
                1,
                "unsent attempt {attempt}, repeat={repeat}"
            );
            assert_eq!(
                notices[0].data["context"]["source_event_ids"],
                json!([mail.id])
            );
            runtime_delivery::append_state(
                &f.home,
                &f.group.group_id,
                "peer",
                &f.group.actors[0].created_at,
                &notices[0].id,
                "pty",
                DeliveryOutcome::Withdrawn("fixture notice withheld before submission"),
            )
            .expect("withdraw unsent notice");
        }
        assert_eq!(f.tick().len(), 1, "Mail still awaits its first handoff");
        assert!(f.tick().is_empty(), "the new notice has one delivery owner");
    }
}

#[test]
fn repeat_mail_respects_resolution_pause_resume_and_actor_generation() {
    for resolution in ["read", "reply", "accepted", "ambiguous"] {
        let f = Fixture::new();
        f.configure(60);
        let mail = f.mail();
        let notice = f.notice(&mail, Some("accepted"));
        match resolution {
            "read" => {
                inbox::consume_unread(&f.home, &f.group, "peer", "peer", 50).expect("read");
            }
            "reply" => {
                f.append(
                    "chat.message",
                    "peer",
                    json!({"to":["user"],"text":"done","reply_to":mail.id}),
                    0,
                );
            }
            outcome => {
                runtime_delivery::append_state(
                    &f.home,
                    &f.group.group_id,
                    "peer",
                    &f.group.actors[0].created_at,
                    &mail.id,
                    "pty",
                    if outcome == "accepted" {
                        DeliveryOutcome::Accepted
                    } else {
                        DeliveryOutcome::Ambiguous("fixture")
                    },
                )
                .expect("manual handoff");
            }
        }
        assert!(f.tick().is_empty(), "resolved by {resolution}");
        assert!(runtime_delivery::is_stale_mail_notice(
            &f.home, &f.group, "peer", &notice
        ));
    }

    let f = Fixture::new();
    f.configure(60);
    let mail = f.mail();
    f.notice(&mail, Some("accepted"));
    for state in [GroupState::Paused, GroupState::Stopped] {
        f.store
            .mutate(&f.group.group_id, |doc| {
                doc.state = state;
                Ok(())
            })
            .expect("pause");
        assert!(f.tick().is_empty());
    }
    f.store
        .mutate(&f.group.group_id, |doc| {
            doc.state = GroupState::Active;
            doc.actors[0].enabled = false;
            Ok(())
        })
        .expect("disable");
    assert!(f.tick().is_empty());
    f.store
        .mutate(&f.group.group_id, |doc| {
            doc.actors[0].enabled = true;
            Ok(())
        })
        .expect("enable");
    f.append("actor.start", "user", json!({"actor_id":"peer"}), 0);
    f.store
        .mutate(&f.group.group_id, |doc| {
            doc.extra
                .get_mut("delivery")
                .and_then(Value::as_object_mut)
                .expect("delivery")
                .insert("mail_notice_after_seconds".into(), json!(60));
            Ok(())
        })
        .expect("resume delay");
    assert!(f.tick().is_empty(), "resume starts a fresh notice window");

    let mut replacement = Actor::new("peer");
    replacement.runtime = ActorRuntime::Custom;
    replacement.submit = ActorSubmit::Newline;
    f.store
        .mutate(&f.group.group_id, |doc| {
            doc.actors[0] = replacement.clone();
            Ok(())
        })
        .expect("new generation");
    f.append("actor.add", "user", json!({"actor":replacement}), 0);
    assert!(
        f.tick().is_empty(),
        "old Mail must not reach a replacement Actor"
    );
}

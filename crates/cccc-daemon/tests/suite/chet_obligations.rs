#![cfg(unix)]

// Included by the crate-level integration test harness.

use cccc_contracts::{ActorRuntime, DaemonError, DaemonRequest, DaemonResponse};
use cccc_core::actors::CHET_SERVICE_PRINCIPAL;
use cccc_core::{GroupStore, HomeLayout, voice_notifications};
use serde_json::{Map, Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
    origin: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let created = call(&home, "group_create", json!({"title": "Chet obligations"}));
        let group_id = created.result["group"]["group_id"]
            .as_str()
            .expect("group id")
            .to_owned();
        let origin = "c".repeat(32);
        voice_notifications::register_origin(
            &home,
            &origin,
            "analyst",
            "thread",
            ActorRuntime::Codex,
        )
        .expect("origin");
        let fixture = Self {
            _temp: temp,
            home,
            group_id,
            origin,
        };
        fixture.ok(
            "actor_add",
            json!({"actor_id": "peer", "runtime": "custom", "by": "user"}),
        );
        fixture
    }

    fn ok(&self, op: &str, args: Value) -> Map<String, Value> {
        let mut args = args.as_object().cloned().expect("args");
        args.insert("group_id".into(), json!(self.group_id));
        call(&self.home, op, Value::Object(args)).result
    }

    fn send(&self, args: Value) -> String {
        self.ok("send", args)["event"]["id"]
            .as_str()
            .expect("id")
            .to_owned()
    }

    fn ask_chet(&self, text: &str) -> String {
        self.send(
            json!({"by": "peer", "to": ["@chet"], "message_mode": "request_reply", "text": text}),
        )
    }

    fn fail(&self, op: &str, args: Value) -> DaemonError {
        let mut args = args.as_object().cloned().expect("args");
        args.insert("group_id".into(), json!(self.group_id));
        let response = cccc_daemon::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        );
        assert!(!response.ok, "{op} should fail: {:?}", response.result);
        response.error.expect("error")
    }

    fn status(&self, event_id: &str) -> Value {
        self.ok("ledger_statuses", json!({"event_ids": [event_id]}))["statuses"][event_id].clone()
    }

    fn obligations(&self, event_id: &str) -> Value {
        self.status(event_id)["obligation_status"].clone()
    }
}

#[test]
fn a_request_to_chet_stays_open_until_chet_replies() {
    let fixture = Fixture::new();
    let request = fixture.ask_chet("Which Group owns the release notes?");
    let open = fixture.obligations(&request);
    assert_eq!(open.as_object().map(Map::len), Some(1), "{open}");
    assert_eq!(
        open[CHET_SERVICE_PRINCIPAL],
        json!({"reply_requested": true, "replied": false, "cancelled": false, "delivery_state": ""})
    );

    let reply = fixture.ok(
        "reply",
        json!({"by": "user", "reply_to": request, "text": "The docs Group.", "_voice_origin": fixture.origin}),
    );
    assert_eq!(reply["event"]["by"], CHET_SERVICE_PRINCIPAL);
    assert_eq!(reply["event"]["data"]["to"], json!(["peer"]));
    let closed = &fixture.obligations(&request)[CHET_SERVICE_PRINCIPAL];
    assert_eq!(closed["replied"], true);
    assert_eq!(closed["cancelled"], false);
}

#[test]
fn a_withdrawn_request_to_chet_closes_without_a_reply() {
    let fixture = Fixture::new();
    let request = fixture.ask_chet("Is the release still blocked?");
    fixture.ok(
        "reply_request_cancel",
        json!({"by": "peer", "source_event_id": request}),
    );
    let closed = &fixture.obligations(&request)[CHET_SERVICE_PRINCIPAL];
    assert_eq!(closed["cancelled"], true);
    assert_eq!(closed["replied"], false);
}

#[test]
fn chet_owes_nothing_for_a_send_or_for_its_own_requests() {
    let fixture = Fixture::new();
    let told =
        fixture.send(json!({"by": "peer", "to": ["@chet"], "message_mode": "send", "text": "FYI"}));
    assert_eq!(
        fixture.obligations(&told)[CHET_SERVICE_PRINCIPAL]["reply_requested"],
        false
    );

    let asked = fixture.send(json!({
        "by": "user", "to": ["peer"], "message_mode": "request_reply",
        "text": "Report the test results.", "_voice_origin": fixture.origin
    }));
    let obligations = fixture.obligations(&asked);
    assert!(
        obligations.get(CHET_SERVICE_PRINCIPAL).is_none(),
        "{obligations}"
    );
    assert_eq!(obligations["peer"]["reply_requested"], true);
}

#[test]
fn the_user_answers_chets_question_in_place_only_while_its_group_is_current() {
    let fixture = Fixture::new();
    let ask = || {
        fixture.send(json!({
            "by": "user", "to": ["user"], "message_mode": "request_reply",
            "text": "The release notes are ready.", "_voice_origin": fixture.origin,
            "question": {"v": 1, "prompt": "Publish them today?",
                         "options": [{"label": "Publish today"}, {"label": "Wait for review"}]}
        }))
    };
    let asked = ask();
    assert_eq!(
        fixture.status(&asked)["user_request"]["requester"],
        "available"
    );
    let answered = fixture.ok(
        "reply_request_answer",
        json!({"source_event_id": asked, "by": "user", "client_id": "chet-a",
               "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(
        answered["event"]["data"]["to"],
        json!([CHET_SERVICE_PRINCIPAL])
    );
    assert_eq!(answered["event"]["data"]["reply_to"], json!(asked));
    assert_eq!(
        fixture.status(&asked)["obligation_status"]["user"]["replied"],
        true
    );

    // A copy or a re-created Group keeps the event, not the identity behind it.
    let earlier = ask();
    let groups = GroupStore::new(fixture.home.clone()).expect("groups");
    let mut group = groups.load(&fixture.group_id).expect("group");
    group.generation = "recreated".into();
    groups.save(&group).expect("new generation");
    assert_eq!(
        fixture.status(&earlier)["user_request"]["requester"],
        "unknown"
    );
    let refused = fixture.fail(
        "reply_request_answer",
        json!({"source_event_id": earlier, "by": "user", "client_id": "chet-b",
               "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(refused.code, "requester_unknown");
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

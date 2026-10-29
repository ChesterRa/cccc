#![cfg(unix)]

// Included by the crate-level integration test harness.

use cccc_contracts::{DaemonError, DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group_id: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let created = call(&home, "group_create", json!({"title": "reply requests"}));
        let group_id = created.result["group"]["group_id"]
            .as_str()
            .expect("group id")
            .to_owned();
        let fixture = Self {
            _temp: temp,
            home,
            group_id,
        };
        fixture.add_actor("lead");
        fixture
    }

    fn add_actor(&self, actor_id: &str) {
        self.ok(
            "actor_add",
            json!({"actor_id": actor_id, "runtime": "custom", "by": "user"}),
        );
    }

    fn args(&self, args: Value) -> Value {
        let mut args = args.as_object().cloned().expect("args");
        args.insert("group_id".into(), json!(self.group_id));
        Value::Object(args)
    }

    fn ok(&self, op: &str, args: Value) -> Map<String, Value> {
        call(&self.home, op, self.args(args)).result
    }

    fn fail(&self, op: &str, args: Value) -> DaemonError {
        let response = handle(&self.home, op, self.args(args));
        assert!(!response.ok, "{op} should fail: {:?}", response.result);
        response.error.expect("error")
    }

    fn ask(&self, question: Value) -> String {
        let sent = self.ok(
            "send",
            json!({
                "by": "lead",
                "to": ["user"],
                "message_mode": "request_reply",
                "text": "The TUI is narrow.",
                "question": question,
            }),
        );
        sent["event"]["id"].as_str().expect("id").to_owned()
    }

    fn ask_plain(&self, text: &str) -> String {
        let sent = self.ok(
            "send",
            json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": text}),
        );
        sent["event"]["id"].as_str().expect("id").to_owned()
    }

    fn answer(&self, source: &str, client_id: &str, answer: Value) -> Map<String, Value> {
        self.ok(
            "reply_request_answer",
            json!({"source_event_id": source, "by": "user", "client_id": client_id, "answer": answer}),
        )
    }

    fn status(&self, event_id: &str) -> Value {
        self.ok("ledger_statuses", json!({"event_ids": [event_id]}))["statuses"][event_id].clone()
    }
}

fn choice() -> Value {
    json!({
        "v": 1,
        "prompt": "Where should the task count go?",
        "options": [
            {"label": "Group page only", "consequence": "the main screen stays simple"},
            {"label": "Add a column"}
        ],
        "recommended": {"option_id": "A", "reason": "60 columns is a hard limit"}
    })
}

#[test]
fn a_question_to_the_user_is_stored_with_the_text_it_generates() {
    let f = Fixture::new();
    let id = f.ask(choice());
    let status = f.status(&id);
    assert_eq!(status["user_request"]["question"], "valid");
    assert_eq!(status["user_request"]["requester"], "available");
    let event = f.ok("ledger_tail", json!({"kind": "chat", "limit": 1}))["events"][0].clone();
    assert_eq!(event["id"], json!(id));
    assert_eq!(event["data"]["question"]["context"], "The TUI is narrow.");
    assert_eq!(event["data"]["question"]["options"][1]["id"], "B");
    assert_eq!(
        event["data"]["text"],
        "The TUI is narrow.\n\nWhere should the task count go?\n\n\
         A. Group page only — the main screen stays simple\n\
         B. Add a column\n\
         Recommended: A — 60 columns is a hard limit\n\n\
         Reply with A, B, or your own answer."
    );

    f.add_actor("peer");
    for (args, code) in [
        (
            json!({"by": "lead", "to": ["peer"], "message_mode": "request_reply", "text": "", "question": choice()}),
            "invalid_question",
        ),
        (
            json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "", "question": choice()}),
            "invalid_question",
        ),
        (
            json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "", "question": {"v": 1, "prompt": "p", "options": [{"label": "only"}]}}),
            "invalid_question",
        ),
        (
            json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "forged", "answer": {"v": 1, "option_id": "A"}}),
            "invalid_args",
        ),
        // Neither could ever be listed among the user's open requests.
        (
            json!({"by": "user", "to": ["user"], "message_mode": "request_reply", "text": "", "question": choice()}),
            "invalid_question",
        ),
        (
            json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "", "question": choice(), "dst_group_id": "g_elsewhere"}),
            "invalid_question",
        ),
    ] {
        assert_eq!(f.fail("send", args.clone()).code, code, "{args}");
    }
    let too_long = f.fail(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "", "question": {"v": 1, "prompt": "é".repeat(501)}}),
    );
    assert_eq!(too_long.details["field"], "question.prompt");
    assert_eq!(too_long.details["limit"], 500);
    assert_eq!(too_long.details["actual"], 501);
    assert_eq!(
        f.fail(
            "reply",
            json!({"by": "lead", "reply_to": id, "text": "", "question": choice()})
        )
        .code,
        "invalid_question"
    );
    assert_eq!(
        f.fail(
            "send_cross_group",
            json!({"by": "lead", "dst_group_id": f.group_id, "to": ["user"], "text": "x", "question": choice()})
        )
        .code,
        "invalid_question"
    );
}

#[test]
fn an_answer_closes_the_request_once_and_reaches_the_requester() {
    let f = Fixture::new();
    let id = f.ask(choice());
    let answered = f.answer(&id, "first", json!({"v": 1, "option_id": "B"}));
    let event = &answered["event"];
    assert_eq!(event["by"], "user");
    assert_eq!(event["data"]["reply_to"], json!(id));
    assert_eq!(event["data"]["to"], json!(["lead"]));
    assert_eq!(event["data"]["text"], "Selected B: Add a column");
    assert_eq!(event["data"]["answer"], json!({"v": 1, "option_id": "B"}));
    // The quote names the question, which a long context would push out.
    assert_eq!(
        event["data"]["quote_text"],
        "Where should the task count go?"
    );
    let status = f.status(&id);
    assert_eq!(status["obligation_status"]["user"]["replied"], true);
    assert_eq!(status["user_request"]["reply_event_id"], event["id"]);

    let retry = f.answer(&id, "first", json!({"v": 1, "option_id": "B"}));
    assert_eq!(retry["duplicate"], true);
    assert_eq!(retry["event"]["id"], event["id"]);
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": id, "client_id": "first", "answer": {"v": 1, "option_id": "A"}})
        )
        .code,
        "idempotency_conflict"
    );
    let closed = f.fail(
        "reply_request_answer",
        json!({"source_event_id": id, "client_id": "second", "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(closed.code, "reply_request_closed");
    assert_eq!(closed.details["state"], "answered");

    let long = f.ok(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "request_reply",
               "text": "Background. ".repeat(40), "question": choice()}),
    );
    let long = long["event"]["id"].as_str().expect("id");
    let answered = f.answer(long, "long", json!({"v": 1, "option_id": "A"}));
    assert_eq!(
        answered["event"]["data"]["quote_text"],
        "Where should the task count go?"
    );

    // Free text is delivered exactly as written.
    let other = f.ask(choice());
    let written = "  both, but the group page first\n";
    let answered = f.answer(&other, "other", json!({"v": 1, "text": written}));
    assert_eq!(
        answered["event"]["data"]["text"],
        format!("Other: {written}")
    );
    assert_eq!(answered["event"]["data"]["answer"]["text"], written);

    // A request without a question gets an ordinary reply in the user's words.
    let plain = f.ask_plain("Which directory should I use?");
    assert_eq!(f.status(&plain)["user_request"]["question"], "none");
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": plain, "client_id": "plain-choice", "answer": {"v": 1, "option_id": "A"}})
        )
        .code,
        "invalid_answer"
    );
    let replied = f.answer(&plain, "plain", json!({"v": 1, "text": "/srv/data"}));
    assert_eq!(replied["event"]["data"]["text"], "/srv/data");
    assert!(replied["event"]["data"].get("answer").is_none());

    let cancelled = f.ask(choice());
    f.ok(
        "reply_request_cancel",
        json!({"source_event_id": cancelled, "by": "lead"}),
    );
    let error = f.fail(
        "reply_request_answer",
        json!({"source_event_id": cancelled, "client_id": "late", "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(error.details["state"], "cancelled");
}

#[test]
fn a_retry_returns_only_the_answer_the_requester_received() {
    let f = Fixture::new();
    f.add_actor("other");
    // An ordinary reply that used the client_id is not this answer, even with
    // the same text: here one to another Actor and one sent as Mail.
    let id = f.ask_plain("Which directory should I use?");
    f.ok(
        "reply",
        json!({"by": "user", "reply_to": id, "to": ["other"], "text": "/srv", "client_id": "to-other"}),
    );
    f.ok(
        "reply",
        json!({"by": "user", "reply_to": id, "to": ["lead"], "message_mode": "mail",
               "text": "/srv", "client_id": "as-mail"}),
    );
    for client_id in ["to-other", "as-mail"] {
        let error = f.fail(
            "reply_request_answer",
            json!({"source_event_id": id, "client_id": client_id, "answer": {"v": 1, "text": "/srv"}}),
        );
        assert_eq!(error.code, "idempotency_conflict");
    }

    // An answer to a question is the structured one, not the same words sent
    // to the requester as an ordinary reply.
    let id = f.ask(choice());
    f.ok(
        "reply",
        json!({"by": "user", "reply_to": id, "to": ["lead"], "text": "both", "client_id": "words"}),
    );
    let error = f.fail(
        "reply_request_answer",
        json!({"source_event_id": id, "client_id": "words", "answer": {"v": 1, "text": "both"}}),
    );
    assert_eq!(error.code, "idempotency_conflict");

    // A stored answer is returned only with the text that answer generates,
    // which is what the requester reads; an imported record may differ.
    let id = f.ask(choice());
    let mut imported = cccc_contracts::Event::new("chat.message", &f.group_id);
    imported.by = "user".into();
    imported.data = json!({
        "reply_to": id, "to": ["lead"], "message_mode": "send", "client_id": "imported",
        "answer": {"v": 1, "option_id": "A"}, "text": "Selected B: Add a column"
    })
    .as_object()
    .cloned()
    .expect("data");
    let store = GroupStore::new(f.home.clone()).expect("store");
    cccc_core::ledger::append(&store.ledger_path(&f.group_id).expect("ledger"), &imported)
        .expect("append");
    let error = f.fail(
        "reply_request_answer",
        json!({"source_event_id": id, "client_id": "imported", "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(error.code, "idempotency_conflict");

    // The answer itself comes back on retry after its requester left.
    let id = f.ask(choice());
    let answered = f.answer(&id, "kept", json!({"v": 1, "option_id": "A"}));
    f.ok("actor_remove", json!({"actor_id": "lead", "by": "user"}));
    let retry = f.answer(&id, "kept", json!({"v": 1, "option_id": "A"}));
    assert_eq!(retry["duplicate"], true);
    assert_eq!(retry["event"]["id"], answered["event"]["id"]);

    // The lookup comes before the request's checks, so a later refusal proves
    // that nothing was recorded. A used client_id whose request cannot be
    // found is unconfirmed rather than refused as a bad source.
    let error = f.fail(
        "reply_request_answer",
        json!({"source_event_id": "missing", "client_id": "kept", "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(error.code, "idempotency_unavailable");
    let error = f.fail(
        "reply_request_answer",
        json!({"source_event_id": "missing", "client_id": "fresh", "answer": {"v": 1, "option_id": "A"}}),
    );
    assert_eq!(error.code, "invalid_source_event");
}

#[test]
fn files_cannot_carry_a_question() {
    let f = Fixture::new();
    let scope = f._temp.path().join("scope");
    std::fs::create_dir_all(&scope).expect("scope");
    std::fs::write(scope.join("plan.txt"), b"plan").expect("file");
    f.ok("attach", json!({"path": scope, "by": "user"}));
    f.ok(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "status", "client_id": "k"}),
    );
    // Refused before a key is looked up or a file is read: neither a used
    // client_id nor a missing file changes the answer.
    for (client_id, path) in [("k", "plan.txt"), ("fresh", "missing.txt")] {
        let error = f.fail(
            "send_files",
            json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "Which plan?",
                   "question": choice(), "client_id": client_id, "paths": [path]}),
        );
        assert_eq!(error.code, "invalid_question");
    }
}

#[test]
fn a_null_question_or_answer_counts_as_absent() {
    let f = Fixture::new();
    let sent = f.ok(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "status",
               "question": null, "answer": null}),
    );
    assert!(sent["event"]["data"].get("answer").is_none());
    let id = sent["event"]["id"].as_str().expect("id");
    let replied = f.ok(
        "reply",
        json!({"by": "user", "reply_to": id, "text": "ok", "question": null, "answer": null}),
    );
    assert!(replied["event"]["data"].get("answer").is_none());
    let error = f.fail(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "status",
               "answer": {"v": 1, "text": "x"}}),
    );
    assert_eq!(error.code, "invalid_args");
}

#[test]
fn an_answer_never_reaches_a_different_registration_of_the_requester() {
    let f = Fixture::new();
    let id = f.ask(choice());
    f.ok("actor_remove", json!({"actor_id": "lead", "by": "user"}));
    assert_eq!(f.status(&id)["user_request"]["requester"], "unavailable");
    f.add_actor("lead");
    assert_eq!(f.status(&id)["user_request"]["requester"], "unavailable");
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": id, "client_id": "a", "answer": {"v": 1, "option_id": "A"}})
        )
        .code,
        "requester_unavailable"
    );

    // An add that stopped after saving the Group but before its event leaves
    // the ledger's latest registration older than the Group's Actor.
    let second = f.ask(choice());
    let store = GroupStore::new(f.home.clone()).expect("store");
    store
        .mutate(&f.group_id, |group| {
            let actor = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == "lead")
                .expect("lead");
            actor.generation = "unrecorded".into();
            Ok(())
        })
        .expect("replace generation");
    assert_eq!(f.status(&second)["user_request"]["requester"], "unknown");
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": second, "client_id": "b", "answer": {"v": 1, "option_id": "A"}})
        )
        .code,
        "requester_unknown"
    );

    // A sender the ledger never registered is unproven, not relayed: only a
    // relay field sends the user to an ordinary reply.
    let ghost = f.ok(
        "send",
        json!({"by": "ghost", "to": ["user"], "message_mode": "request_reply", "text": "Which port?"}),
    );
    let ghost = ghost["event"]["id"].as_str().expect("id");
    let status = f.status(ghost);
    assert_eq!(status["user_request"]["requester"], "unknown");
    assert!(status["user_request"].get("reply_route").is_none());
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": ghost, "client_id": "c", "answer": {"v": 1, "text": "8080"}})
        )
        .code,
        "requester_unknown"
    );
}

#[test]
fn an_answer_leaves_a_paused_or_stopped_group_and_a_disabled_requester_as_they_are() {
    fn lead_enabled(group: &Map<String, Value>) -> Value {
        group["group"]["actors"]
            .as_array()
            .and_then(|actors| actors.iter().find(|actor| actor["id"] == "lead"))
            .map(|actor| actor["enabled"].clone())
            .expect("lead")
    }
    for case in ["paused", "stopped", "disabled"] {
        let f = Fixture::new();
        let question = f.ask(choice());
        let plain = f.ask_plain("Which port?");
        match case {
            "paused" => f.ok("group_set_state", json!({"by": "user", "state": "paused"})),
            "stopped" => f.ok("group_stop", json!({"by": "user"})),
            _ => f.ok(
                "actor_update",
                json!({"actor_id": "lead", "by": "user", "patch": {"enabled": false}}),
            ),
        };
        let before = f.ok("group_show", json!({}));
        f.answer(&question, "choice", json!({"v": 1, "option_id": "A"}));
        f.answer(&plain, "words", json!({"v": 1, "text": "8080"}));
        let after = f.ok("group_show", json!({}));
        assert_eq!(after["group"]["state"], before["group"]["state"], "{case}");
        assert_eq!(
            after["group"]["running"], before["group"]["running"],
            "{case}"
        );
        assert_eq!(lead_enabled(&after), lead_enabled(&before), "{case}");
        // Both are recorded and closed; delivery waits until the user resumes.
        for id in [&question, &plain] {
            assert_eq!(
                f.status(id)["obligation_status"]["user"]["replied"],
                true,
                "{case}"
            );
        }
    }
}

#[test]
fn a_request_relayed_from_another_group_is_answered_with_an_ordinary_reply() {
    let f = Fixture::new();
    let created = call(&f.home, "group_create", json!({"title": "elsewhere"}));
    let elsewhere = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    let sent = f.ok(
        "send_cross_group",
        json!({"by": "lead", "dst_group_id": elsewhere, "to": ["user"],
               "message_mode": "request_reply", "text": "Which port?"}),
    );
    let relayed = sent["dst_event"]["id"].as_str().expect("relayed id");
    let statuses = call(
        &f.home,
        "ledger_statuses",
        json!({"group_id": elsewhere, "event_ids": [relayed]}),
    );
    let request = &statuses.result["statuses"][relayed]["user_request"];
    assert_eq!(request["requester"], "unsupported");
    assert_eq!(request["reply_route"], "composer");
    let refused = handle(
        &f.home,
        "reply_request_answer",
        json!({"group_id": elsewhere, "source_event_id": relayed, "by": "user",
               "client_id": "r", "answer": {"v": 1, "text": "8080"}}),
    );
    let error = refused.error.expect("refused");
    assert_eq!(error.code, "unsupported_request_source");
    assert_eq!(error.details["reply_route"], "composer");
}

#[test]
fn a_question_that_no_longer_matches_its_text_is_answered_only_in_words() {
    let f = Fixture::new();
    let id = f.ask(choice());
    let event = f.ok("ledger_tail", json!({"kind": "chat", "limit": 1}))["events"][0].clone();
    let mut copy = event.clone();
    copy["id"] = json!("imported-request");
    copy["data"]["text"] = json!("An edited text that offers different choices");
    f.ok("event_append", json!({"event": copy}));
    assert_eq!(f.status(&id)["user_request"]["question"], "valid");
    assert_eq!(
        f.status("imported-request")["user_request"]["question"],
        "unavailable"
    );
    assert_eq!(
        f.fail(
            "reply_request_answer",
            json!({"source_event_id": "imported-request", "client_id": "c", "answer": {"v": 1, "option_id": "A"}})
        )
        .code,
        "question_unavailable"
    );
    let replied = f.answer(
        "imported-request",
        "d",
        json!({"v": 1, "text": "Neither; ask again"}),
    );
    assert_eq!(replied["event"]["data"]["text"], "Neither; ask again");
    assert!(replied["event"]["data"].get("answer").is_none());
}

#[test]
fn only_the_user_reads_and_pages_open_requests() {
    let f = Fixture::new();
    // Only a missing `by` means the user; a wrong type never defaults to it.
    for by in [
        json!("lead"),
        json!(null),
        json!(""),
        json!(1),
        json!(["user"]),
    ] {
        for (op, args) in [
            ("user_reply_requests", json!({"by": by})),
            ("user_reply_requests_summary", json!({"by": by})),
            (
                "reply_request_answer",
                json!({"by": by, "source_event_id": "x", "client_id": "x", "answer": {"v": 1, "text": "x"}}),
            ),
        ] {
            assert_eq!(f.fail(op, args).code, "permission_denied", "{op} {by}");
        }
    }
    let ids = (0..3)
        .map(|index| f.ask_plain(&format!("request {index}")))
        .collect::<Vec<_>>();
    let first = f.ok("user_reply_requests", json!({"by": "user", "limit": 2}));
    assert_eq!(first["total"], 3);
    assert_eq!(first["has_more"], true);
    assert_eq!(
        first["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|event| event["id"].clone())
            .collect::<Vec<_>>(),
        [json!(ids[1]), json!(ids[2])]
    );
    assert_eq!(
        first["statuses"][&ids[2]]["user_request"]["requester"],
        "available"
    );
    f.answer(&ids[1], "close-cursor", json!({"v": 1, "text": "done"}));
    let second = f.ok(
        "user_reply_requests",
        json!({"before": first["before"], "limit": 2}),
    );
    assert_eq!(second["events"][0]["id"], json!(ids[0]));
    assert_eq!(second["has_more"], false);
    assert_eq!(
        f.fail("user_reply_requests", json!({"before": "v1.bad"}))
            .code,
        "invalid_cursor"
    );

    let summary = f.ok(
        "user_reply_requests_summary",
        json!({"group_ids": [f.group_id, "g_missing"]}),
    );
    assert_eq!(summary["groups"][0]["state"], "fresh");
    assert_eq!(summary["groups"][0]["pending"], 2);
    assert_eq!(summary["groups"][1]["state"], "unknown");
    assert!(summary["groups"][1]["pending"].is_null());

    let tail = f.ok(
        "ledger_tail",
        json!({"kind": "chat", "limit": 1, "with_obligation_status": true}),
    );
    // The latest chat event is the user's reply; the request before it carries
    // the user's side of its obligation.
    let window = f.ok(
        "ledger_tail",
        json!({"kind": "chat", "limit": 5, "with_obligation_status": true}),
    );
    assert!(tail["events"][0].get("_user_request").is_none());
    assert!(
        window["events"]
            .as_array()
            .expect("events")
            .iter()
            .any(|event| event["id"] == json!(ids[2])
                && event["_user_request"]["question"] == "none")
    );
}

fn handle(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = handle(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

#[test]
fn a_damaged_line_makes_the_users_requests_unknown_instead_of_open() {
    let damaged: [&[u8]; 3] = [
        b"{\"kind\": \"chat.reply_request.cancelled\", broken\n",
        b"{\"not\": \"an event\"}\n",
        b"{\"kind\": \"chat.reply_request.cancelled\", \"data\": {}}\n",
    ];
    for line in damaged {
        let f = Fixture::new();
        let id = f.ask(choice());
        let ledger = GroupStore::new(f.home.clone())
            .expect("store")
            .ledger_path(&f.group_id)
            .expect("ledger");
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&ledger)
                .expect("ledger");
            file.write_all(line).expect("damaged line");
        }
        let summary = f.ok(
            "user_reply_requests_summary",
            json!({"group_ids": [f.group_id]}),
        );
        assert_eq!(
            summary["groups"][0]["state"],
            "unknown",
            "{}",
            String::from_utf8_lossy(line)
        );
        assert_eq!(
            f.fail("user_reply_requests", json!({})).code,
            "ledger_unreadable"
        );
        assert_eq!(
            f.fail(
                "reply_request_answer",
                json!({"source_event_id": id, "client_id": "after-damage", "answer": {"v": 1, "option_id": "A"}})
            )
            .code,
            "ledger_unreadable"
        );
        // The damaged record could be the first send of a retried question.
        assert_eq!(
            f.fail(
                "send",
                json!({"by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "",
                       "client_id": "after-damage", "question": {"v": 1, "prompt": "Which port?"}})
            )
            .code,
            "idempotency_unavailable"
        );
    }
}

#[test]
fn a_question_reuses_its_client_id_only_for_the_same_question() {
    let f = Fixture::new();
    let plain = f.ok(
        "send",
        json!({"by": "lead", "to": ["user"], "message_mode": "send", "text": "status", "client_id": "k1"}),
    );
    let question = |client_id: &str, prompt: &str| {
        json!({
            "by": "lead", "to": ["user"], "message_mode": "request_reply", "text": "",
            "client_id": client_id,
            "question": {"v": 1, "prompt": prompt},
        })
    };
    // An earlier, different message under the same client_id is a conflict,
    // not a question that was already created.
    assert_eq!(
        f.fail("send", question("k1", "Which directory?")).code,
        "idempotency_conflict"
    );
    let first = f.ok("send", question("k2", "Which directory?"));
    let retry = f.ok("send", question("k2", "Which directory?"));
    assert_eq!(retry["duplicate"], true);
    assert_eq!(retry["event"]["id"], first["event"]["id"]);
    assert_eq!(
        f.fail("send", question("k2", "Which port?")).code,
        "idempotency_conflict"
    );
    // A retry repeats the whole request, not only its question: a mode or
    // audience a question cannot have is refused as such, and any other
    // change to what was sent conflicts.
    f.add_actor("peer");
    for (field, value, code) in [
        ("message_mode", json!("send"), "invalid_question"),
        ("to", json!(["peer"]), "invalid_question"),
        (
            "insight",
            json!("The port is taken."),
            "idempotency_conflict",
        ),
    ] {
        let mut changed = question("k2", "Which directory?");
        changed[field] = value;
        assert_eq!(f.fail("send", changed).code, code, "{field}");
    }
    assert_ne!(first["event"]["id"], plain["event"]["id"]);
    // The key is looked up and stored as the same trimmed value, and a key
    // given explicitly must be one.
    let padded = f.ok("send", question(" k3 ", "Which branch?"));
    for client_id in [" k3 ", "k3"] {
        let retry = f.ok("send", question(client_id, "Which branch?"));
        assert_eq!(retry["duplicate"], true, "{client_id:?}");
        assert_eq!(retry["event"]["id"], padded["event"]["id"]);
    }
    for client_id in [json!(""), json!("  "), json!(7)] {
        let mut invalid = question("unused", "Which tag?");
        invalid["client_id"] = client_id.clone();
        assert_eq!(f.fail("send", invalid).code, "invalid_args", "{client_id}");
    }
    let open = f.ok("user_reply_requests", json!({}));
    assert_eq!(open["total"], 2);
}

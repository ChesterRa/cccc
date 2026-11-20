//! Turn outcomes through the actual managed Actor message entry point.
//! The provider transport is inert; these tests create no provider processes.

use super::*;
use crate::ops::codex_voice_analyst::AnalystSession;
use cccc_core::{GroupStore, HomeLayout};
use std::sync::{Arc, Mutex, atomic::AtomicBool};

struct Fixture {
    session: Session,
    _directory: crate::ops::local_headless::supervisor::control_fixture::ControlDirectory,
    _listener: tokio::net::UnixListener,
    _temp: tempfile::TempDir,
}

impl Fixture {
    fn new(structured: bool, private: &str) -> Self {
        let temp = tempfile::tempdir().expect("isolated output fixture");
        let config = temp.path().canonicalize().expect("canonical fixture path");
        let (listener, directory) =
            crate::ops::local_headless::supervisor::control_fixture::bind(&config);
        let home = HomeLayout::from_path(config.join("home")).expect("isolated home");
        let store = GroupStore::new(home.clone()).expect("isolated group store");
        let group = store
            .create("output outcome fixture", "")
            .expect("fixture group");
        // A private member setting, as the launch snapshot delivers it.
        let snapshot = std::collections::BTreeMap::from([(
            "OUTPUT_FIXTURE_PRIVATE".to_owned(),
            private.to_owned(),
        )]);
        let journal_private =
            journal_private_values(&snapshot, &BTreeMap::new(), std::iter::empty());
        let session = Session {
            home,
            group_id: group.group_id,
            actor_id: "fixture-actor".into(),
            workspace: Default::default(),
            managed: Arc::new(AnalystSession::for_output_test(
                &config, structured, private,
            )),
            has_terminal: AtomicBool::new(false),
            viewer: Mutex::new(None),
            status: Mutex::new(super::super::HeadlessStatus {
                status: "idle".into(),
                task_id: None,
                updated_at: String::new(),
                pid: None,
                reason: None,
            }),
            stopped: AtomicBool::new(false),
            stop_lock: Mutex::new(()),
            startup_prompt: Mutex::new(None),
            active_turn: Mutex::new(None),
            journal_private,
        };
        Self {
            session,
            _directory: directory,
            _listener: listener,
            _temp: temp,
        }
    }

    fn start(&self, turn_id: &str) {
        handle_message(
            &self.session,
            json!({"method":"turn/started","params":{"threadId":"abcdef01","turn":{"id":turn_id}}}),
        );
    }

    fn complete(&self, turn_id: &str, status: &str, error: Value) {
        handle_message(
            &self.session,
            json!({"method":"turn/completed","params":{"threadId":"abcdef01","turn":{"id":turn_id,"status":status,"error":error}}}),
        );
    }

    fn events(&self) -> Vec<Value> {
        let path = GroupStore::new(self.session.home.clone())
            .expect("read fixture store")
            .state_dir(&self.session.group_id)
            .expect("fixture state dir")
            .join("headless/events.jsonl");
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("normalized journal event"))
            .collect()
    }

    fn terminals(&self) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|event| {
                matches!(
                    event["type"].as_str(),
                    Some(
                        "headless.turn.completed"
                            | "headless.turn.failed"
                            | "headless.turn.cancelled"
                    )
                )
            })
            .collect()
    }
}

#[tokio::test]
async fn managed_failure_keeps_ordinary_inherited_values_and_redacts_credentials() {
    const MEMBER: &str = "member-pin-4821";
    const TOKEN: &str = "inherited-opaque-value";
    const AUTH_PROXY: &str = "http://probe-user:probe-password@proxy.example.test:8080";
    const AUTH_AUTHORITY: &str = "probe-user:probe-password@bare.example.test:8080";
    const PUBLIC_PROXY: &str = "https://proxy.example.test:443";
    const INHERITED_URI: &str = "redis://fixture:fixture-redis-password@cache.example.test/0";
    const CONFIGURED_PATH: &str = "/private/first:/bin/cccc:/private/last";
    const LAUNCHED_PATH: &str = "/bin/cccc:/private/first:/private/last";
    let extra_credentials = [
        ("PGPASSWORD", "fixture-postgres-password"),
        ("MYSQL_PWD", "fixture-mysql-password"),
        ("npm_config__auth", "fixture-npm-basic-auth"),
        (
            "npm_config_//registry.example.test/:_auth",
            "fixture-scoped-npm-auth",
        ),
        ("DOCKER_AUTH_CONFIG", "fixture-docker-auth-config"),
        ("GIT_CONFIG_VALUE_0", "Authorization: Basic ZmljdHVyZQ=="),
        ("VENDOR_ENDPOINT", INHERITED_URI),
    ];
    let ordinary = format!(
        "attempt 1 of the dumb fixture {PUBLIC_PROXY} /workspace/fixture author@example.test /inherited/bin"
    );
    let mut f = Fixture::new(true, MEMBER);
    let snapshot = BTreeMap::from([
        ("DEPLOY_PIN".to_owned(), MEMBER.to_owned()),
        ("PATH".to_owned(), CONFIGURED_PATH.to_owned()),
    ]);
    let launch_environment = BTreeMap::from([("PATH".to_owned(), LAUNCHED_PATH.to_owned())]);
    let inherited = [
        ("SHLVL", "1"),
        ("TERM", "dumb"),
        ("GH_TOKEN", TOKEN),
        ("http_proxy", AUTH_PROXY),
        ("HTTPS_PROXY", AUTH_AUTHORITY),
        ("ALL_PROXY", PUBLIC_PROXY),
        ("PWD", "/workspace/fixture"),
        ("GIT_AUTHOR_EMAIL", "author@example.test"),
        ("PATH", "/inherited/bin"),
    ];
    f.session.journal_private = journal_private_values(
        &snapshot,
        &launch_environment,
        inherited
            .into_iter()
            .chain(extra_credentials)
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
    f.start("turn-origin");
    let credentials = [
        MEMBER,
        TOKEN,
        AUTH_PROXY,
        AUTH_AUTHORITY,
        CONFIGURED_PATH,
        LAUNCHED_PATH,
    ]
    .into_iter()
    .chain(extra_credentials.map(|(_, value)| value))
    .collect::<Vec<_>>();
    let diagnostic = format!("{ordinary}\n{}", credentials.join(" "));
    f.complete("turn-origin", "failed", json!({"message": diagnostic}));
    let expected = format!(
        "{ordinary}\n{}[redacted]",
        "[redacted] ".repeat(credentials.len() - 1)
    );
    assert_eq!(f.terminals()[0]["data"]["error"], expected);
    assert_eq!(
        f.session.status.lock().expect("state").reason.as_deref(),
        Some(expected.as_str())
    );
}

#[tokio::test]
async fn structured_failure_has_one_terminal_and_clears_on_the_next_turn() {
    let f = Fixture::new(true, "");
    f.start("turn-1");
    f.complete("turn-1", "failed", json!("fixture provider failure"));
    f.complete("turn-1", "failed", json!("duplicate provider notification"));
    let terminals = f.terminals();
    assert_eq!(terminals.len(), 1, "one failure has one journal terminal");
    assert_eq!(terminals[0]["type"], "headless.turn.failed");
    assert_eq!(terminals[0]["data"]["stream_id"], "abcdef01:turn-1");
    assert_eq!(terminals[0]["data"]["status"], "failed");
    assert_eq!(
        f.events()
            .iter()
            .filter(|event| event["type"] == "headless.message.completed")
            .count(),
        1,
        "message and turn are each closed once"
    );
    assert_eq!(f.session.status.lock().expect("state").status, "error");
    assert!(f.session.active_turn.lock().expect("active").is_none());
    f.start("turn-2");
    assert_eq!(f.session.status.lock().expect("state").status, "working");
    assert!(f.session.status.lock().expect("state").reason.is_none());
    f.complete("turn-2", "completed", Value::Null);
    assert_eq!(f.terminals().len(), 2);
    assert_eq!(f.terminals()[1]["type"], "headless.turn.completed");
    assert_eq!(f.session.status.lock().expect("state").status, "idle");
    assert!(f.session.status.lock().expect("state").reason.is_none());
}

#[tokio::test]
async fn structured_cancellation_closes_once_without_a_failure() {
    for status in ["cancelled", "canceled", "interrupted"] {
        let f = Fixture::new(true, "");
        f.start("turn-1");
        f.complete("turn-1", status, json!("operator interruption"));
        let terminals = f.terminals();
        assert_eq!(terminals.len(), 1, "cancellation is one terminal outcome");
        assert_eq!(terminals[0]["type"], "headless.turn.cancelled");
        assert_eq!(terminals[0]["data"]["stream_id"], "abcdef01:turn-1");
        assert_eq!(terminals[0]["data"]["status"], "cancelled");
        assert!(terminals[0]["data"]["error"].is_null());
        assert_eq!(f.session.status.lock().expect("state").status, "idle");
        assert!(f.session.status.lock().expect("state").reason.is_none());
    }
}

#[tokio::test]
async fn foreign_turn_and_thread_frames_do_not_change_the_owned_stream() {
    let f = Fixture::new(true, "");
    f.start("turn-main");
    let before = f.events();
    for message in [
        json!({"method":"turn/completed","params":{"threadId":"abcdef01","turn":{"id":"turn-old","status":"failed","error":"old failure"}}}),
        json!({"method":"item/agentMessage/delta","params":{"threadId":"abcdef01","turnId":"turn-old","delta":"old progress"}}),
        json!({"method":"turn/started","params":{"threadId":"foreign-thread","turn":{"id":"turn-sub"}}}),
        json!({"method":"turn/completed","params":{"threadId":"foreign-thread","turn":{"id":"turn-main","status":"failed","error":"sub-agent failure"}}}),
        json!({"method":"item/agentMessage/delta","params":{"threadId":"foreign-thread","turnId":"turn-main","delta":"sub-agent progress"}}),
    ] {
        handle_message(&f.session, message);
    }
    assert_eq!(
        f.events(),
        before,
        "foreign frames must not reach the journal"
    );
    assert!(
        f.session
            .active_turn
            .lock()
            .expect("active")
            .as_ref()
            .is_some_and(|turn| turn.turn_id == "turn-main")
    );
    assert_eq!(f.session.status.lock().expect("state").status, "working");
}

#[tokio::test]
async fn native_failure_and_recovery_preserve_native_completion_semantics() {
    let f = Fixture::new(false, "");
    f.start("turn-1");
    f.complete("turn-1", "failed", json!("fixture provider failure"));
    assert!(f.session.active_turn.lock().expect("active").is_none());
    assert_eq!(f.session.status.lock().expect("state").status, "error");
    assert!(
        f.session.status.lock().expect("state").reason.as_deref()
            == Some("fixture provider failure")
    );
    assert_eq!(f.terminals().len(), 1);
    assert_eq!(f.terminals()[0]["data"]["stream_id"], "abcdef01:turn-1");
    f.start("turn-2");
    assert_eq!(f.session.status.lock().expect("state").status, "working");
    assert!(f.session.status.lock().expect("state").reason.is_none());
    f.complete("turn-2", "completed", Value::Null);
    assert_eq!(f.session.status.lock().expect("state").status, "idle");
    assert!(f.session.status.lock().expect("state").reason.is_none());
    assert_eq!(
        f.terminals().len(),
        1,
        "native success still only updates status"
    );
    assert!(
        f.events()
            .iter()
            .all(|event| event["type"] != "headless.message.completed")
    );
}

#[tokio::test]
async fn progress_for_a_settled_turn_does_not_reopen_its_stream() {
    let f = Fixture::new(true, "");
    f.start("turn-1");
    f.complete("turn-1", "completed", Value::Null);
    let before = f.events();
    handle_message(
        &f.session,
        json!({"method":"item/agentMessage/delta","params":{"threadId":"abcdef01","turnId":"turn-1","delta":"late provider progress"}}),
    );
    assert_eq!(
        f.events(),
        before,
        "settled progress must not revive a stream"
    );
    assert!(f.session.active_turn.lock().expect("active").is_none());
    assert_eq!(f.session.status.lock().expect("state").status, "idle");
}

#[tokio::test]
async fn failed_outcomes_share_one_bounded_private_safe_diagnostic() {
    const PRIVATE: &str = "Synthetic-PRIVATE-Setting-7B35e99B05";
    const OPAQUE: &str = "unselected-provider-payload";
    for structured in [false, true] {
        let f = Fixture::new(structured, PRIVATE);
        f.start("turn-private");
        f.complete(
            "turn-private",
            "failed",
            json!({"message":format!("Provider rejected {PRIVATE}: {} {PRIVATE}", "界".repeat(1700)), "data":{"unselected":OPAQUE}}),
        );
        let reason = f
            .session
            .status
            .lock()
            .expect("state")
            .reason
            .clone()
            .expect("safe reason");
        assert!(
            !reason.contains(PRIVATE),
            "private setting must be redacted"
        );
        assert!(
            reason.contains("[redacted]"),
            "diagnostic retains safe redaction context"
        );
        assert_eq!(reason.chars().count(), 1600);
        assert!(
            f.terminals()[0]["data"]["error"].as_str() == Some(reason.as_str()),
            "status and journal must use the same sanitized diagnostic"
        );
        let journal = serde_json::to_string(&f.events()).expect("fixture journal JSON");
        assert!(
            !journal.contains(PRIVATE),
            "journal must not expose a private setting"
        );
        assert!(
            !journal.contains(OPAQUE),
            "journal must not expose arbitrary provider data"
        );

        f.start("turn-opaque");
        f.complete(
            "turn-opaque",
            "failed",
            json!({"opaque":{"private":PRIVATE,"data":OPAQUE}}),
        );
        let reason = f
            .session
            .status
            .lock()
            .expect("state")
            .reason
            .clone()
            .expect("generic reason");
        assert!(
            !reason.contains(PRIVATE) && !reason.contains(OPAQUE) && !reason.contains("opaque"),
            "unknown error JSON must become a generic diagnostic"
        );

        f.start("turn-status");
        f.complete("turn-status", PRIVATE, Value::Null);
        let journal = serde_json::to_string(&f.events()).expect("fixture journal JSON");
        assert!(
            !journal.contains(PRIVATE) && !journal.contains(&PRIVATE.to_ascii_lowercase()),
            "unknown provider status must not bypass redaction"
        );
        assert_eq!(f.terminals()[2]["data"]["status"], "failed");
    }
}

#[tokio::test]
async fn completed_with_an_error_is_projected_as_one_failed_outcome() {
    let f = Fixture::new(true, "");
    f.start("turn-1");
    f.complete("turn-1", "completed", json!("partial provider failure"));
    let terminals = f.terminals();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0]["type"], "headless.turn.failed");
    assert_eq!(terminals[0]["data"]["status"], "failed");
    assert_eq!(f.session.status.lock().expect("state").status, "error");
}

#[tokio::test]
async fn a_captured_pi_length_stop_without_a_reply_reads_as_an_error() {
    // A real Pi turn that hit the model's length limit before writing any text.
    let f = Fixture::new(true, "");
    let captured = crate::ops::codex_voice_analyst::CAPTURED_PI_LENGTH_STOP;
    for message in crate::ops::codex_voice_analyst::replay_captured_pi_turn(captured, "abcdef01") {
        handle_message(&f.session, message);
    }
    let terminals = f.terminals();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0]["type"], "headless.turn.failed");
    let error = terminals[0]["data"]["error"].as_str().unwrap_or_default();
    assert!(error.contains("length limit"), "{error}");
    let state = f.session.status.lock().expect("state");
    assert_eq!(state.status, "error");
    assert_eq!(state.reason.as_deref(), Some(error));
}

#[tokio::test]
async fn captured_pi_tool_calls_reach_the_journal_between_turn_start_and_answer() {
    // The captured session reads notes.txt. Marking that value private proves
    // that call arguments pass the session's redaction before the journal.
    let f = Fixture::new(true, "notes.txt");
    let captured = crate::ops::codex_voice_analyst::CAPTURED_PI_TOOL_TURN;
    for message in crate::ops::codex_voice_analyst::replay_captured_pi_turn(captured, "abcdef01") {
        handle_message(&f.session, message);
    }
    let events = f.events();
    let kinds = events
        .iter()
        .filter_map(|event| event["type"].as_str())
        .filter(|kind| *kind != "headless.message.delta")
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "headless.turn.started",
            "headless.activity.started",
            "headless.activity.started",
            "headless.activity.completed",
            "headless.activity.completed",
            "headless.message.completed",
            "headless.turn.completed",
        ]
    );
    let activity = |kind: &str, tool: &str| {
        events
            .iter()
            .find(|event| event["type"] == kind && event["data"]["tool_name"] == tool)
            .map(|event| event["data"].clone())
            .expect("tool activity")
    };
    let bash = activity("headless.activity.completed", "bash");
    assert_eq!(bash["status"], "completed");
    assert_eq!(bash["kind"], "tool");
    assert_eq!(bash["raw_item_type"], "commandExecution");
    assert_eq!(bash["command"], "ls", "the program, not its arguments");
    assert_eq!(
        bash["activity_id"],
        activity("headless.activity.started", "bash")["activity_id"],
        "a call's start and finish share one trace entry"
    );
    let read = activity("headless.activity.started", "read");
    assert_eq!(read["status"], "started");
    assert_eq!(read["detail"], "[redacted]");
    let text = events
        .iter()
        .filter(|event| event["type"] == "headless.message.delta")
        .filter_map(|event| event["data"]["delta"].as_str())
        .collect::<String>();
    // Both assistant messages stream: the line before the calls, then the answer.
    assert_eq!(
        text,
        "I'll run the command and read the file.notes.txt contains the text \"fixture file\"."
    );
}

#[tokio::test]
async fn named_tool_activity_finishes_without_journaling_transient_or_foreign_updates() {
    let f = Fixture::new(true, "private-token");
    f.start("turn-1");
    let tool_event = |turn: &str, phase: Option<&str>| {
        let mut params = json!({
            "threadId":"abcdef01", "turnId":turn, "title":"bash",
            "detail":"cargo test private-token", "itemType":"commandExecution"
        });
        if let Some(phase) = phase {
            params["itemId"] = json!("call-1");
            params["phase"] = json!(phase);
        }
        json!({"method":"cccc/toolActivity", "params":params})
    };
    handle_message(&f.session, tool_event("turn-1", None));
    handle_message(&f.session, tool_event("another-turn", Some("started")));
    handle_message(&f.session, tool_event("turn-1", Some("started")));
    handle_message(&f.session, tool_event("turn-1", None));
    handle_message(&f.session, tool_event("turn-1", Some("failed")));
    let events = f.events();
    let activity = events
        .iter()
        .filter(|event| {
            event["type"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("headless.activity."))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        activity.len(),
        2,
        "only the identified call's start and end are journaled"
    );
    assert_eq!(activity[0]["type"], "headless.activity.started");
    assert_eq!(activity[1]["type"], "headless.activity.completed");
    assert_eq!(
        activity[0]["data"]["activity_id"],
        activity[1]["data"]["activity_id"]
    );
    assert_eq!(activity[1]["data"]["summary"], "Failed: bash");
    assert_eq!(activity[1]["data"]["command"], "cargo test [redacted]");
    assert_eq!(
        f.session.status.lock().expect("state").status,
        "working",
        "a failed tool can be handled by the model; it does not end the turn"
    );
    f.complete("turn-1", "completed", Value::Null);
    assert_eq!(f.terminals().len(), 1);
}

#[tokio::test]
async fn a_tool_title_is_redacted_before_it_is_shortened() {
    let f = Fixture::new(true, "private-token");
    f.start("turn-1");
    // The 400-character cut falls inside the private value: shortening first
    // would keep a prefix that redaction no longer recognizes.
    let title = format!("{}private-token and the rest", "a".repeat(395));
    handle_message(
        &f.session,
        json!({"method":"cccc/toolActivity", "params":{
            "threadId":"abcdef01", "turnId":"turn-1", "title":title,
            "itemId":"call-1", "phase":"completed"
        }}),
    );
    let events = f.events();
    let activity = events
        .iter()
        .filter(|event| {
            event["type"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("headless.activity."))
        })
        .collect::<Vec<_>>();
    assert_eq!(activity.len(), 1, "a finish without a start is journaled");
    assert_eq!(activity[0]["type"], "headless.activity.completed");
    assert_eq!(activity[0]["data"]["activity_id"], "turn-1:call-1");
    for field in ["summary", "tool_name"] {
        let text = activity[0]["data"][field].as_str().expect("title field");
        assert_eq!(text.chars().count(), 400, "{field}");
        assert!(text.starts_with(&"a".repeat(395)), "{field}");
        assert!(
            !text.contains("priv"),
            "{field} keeps part of the private value: {text}"
        );
    }
}

#[tokio::test]
async fn the_journal_redacts_values_by_origin_and_keeps_ordinary_text() {
    let mut f = Fixture::new(true, "");
    let pairs = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    // The launch snapshot: the member's settings, a private PATH among them,
    // and the fields the launch overwrites.
    let snapshot = pairs(&[
        ("DEPLOY_PIN", "4821"),
        ("NOTES_DIR", "/srv/private/notes"),
        ("PATH", "/srv/private/bin"),
        ("CCCC_HOME", "/home/u/.cccc"),
        ("CCCC_GROUP_ID", "g_fixture"),
        ("CCCC_ACTOR_ID", "fixture-actor"),
        ("CCCC_CLI", "/opt/cccc/cccc"),
        ("EMPTY", ""),
    ]);
    // Inherited values count only under a credential name.
    let inherited = pairs(&[
        ("SHLVL", "0"),
        ("HOME", "/home/u"),
        ("LANG", "C.UTF-8"),
        ("PI_SKIP_VERSION_CHECK", "1"),
        ("PI_TELEMETRY", "0"),
        ("GH_TOKEN", "private-token"),
    ]);
    f.session.journal_private = journal_private_values(&snapshot, &BTreeMap::new(), inherited);
    assert_eq!(
        f.session.journal_private,
        [
            "/srv/private/bin",
            "/srv/private/notes",
            "4821",
            "private-token"
        ]
    );

    f.start("turn-1");
    handle_message(
        &f.session,
        json!({"method":"cccc/toolActivity", "params":{
            "threadId":"abcdef01", "turnId":"turn-1", "itemId":"call-1", "phase":"failed",
            "title":"Read file1.txt --timeout 600 --pin 4821 --auth private-token /home/u/.cccc/groups/g_fixture/fixture-actor.md /srv/private/notes /srv/private/bin/deploy"
        }}),
    );
    let events = f.events();
    let finished = events
        .iter()
        .find(|event| event["type"] == "headless.activity.completed")
        .expect("finished call");
    let shown = "Read file1.txt --timeout 600 --pin [redacted] --auth [redacted] /home/u/.cccc/groups/g_fixture/fixture-actor.md [redacted] [redacted]/deploy";
    assert_eq!(finished["data"]["tool_name"], shown);
    assert_eq!(finished["data"]["summary"], format!("Failed: {shown}"));
}

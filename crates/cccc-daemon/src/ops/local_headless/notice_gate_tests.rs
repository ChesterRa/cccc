//! Early Mail-notice admission against a managed session driven by real turn
//! events delivered through the provider reader's entry point.

use super::*;
use crate::ops::codex_voice_analyst::AnalystSession;
use serde_json::json;

// The Claude fixture resolves a control socket from its config directory;
// keep the directory for as long as the session lives.
fn session(temp: &std::path::Path) -> (Session, super::control_fixture::ControlDirectory) {
    let config = temp.canonicalize().expect("config");
    let (_listener, directory) = super::control_fixture::bind(&config);
    let session = Session {
        home: HomeLayout::from_path(config.join("home")).expect("home"),
        group_id: "g_notice".into(),
        actor_id: "peer".into(),
        managed: Arc::new(AnalystSession::claude_for_shutdown_test(
            &config,
            "abcdef09",
            Vec::new(),
            false,
        )),
        has_terminal: AtomicBool::new(true),
        viewer: Mutex::new(None),
        status: Mutex::new(HeadlessStatus {
            status: "idle".into(),
            reason: None,
            task_id: None,
            updated_at: String::new(),
            pid: None,
        }),
        stopped: AtomicBool::new(false),
        stop_lock: Mutex::new(()),
        startup_prompt: Mutex::new(None),
        active_turn: Mutex::new(None),
    };
    (session, directory)
}

fn event(kind: &str, data: serde_json::Value) -> Event {
    let mut event = Event::new(kind, "g_notice");
    event.data = data.as_object().cloned().expect("data");
    event
}

fn notice_due_in(seconds: i64) -> Event {
    let deliver_by = chrono::Utc::now() + chrono::Duration::seconds(seconds);
    event(
        "system.notify",
        json!({"kind":"mail_notice","context":{"deliver_by":deliver_by.to_rfc3339()}}),
    )
}

/// An early notice still within its hold window.
fn notice() -> Event {
    notice_due_in(3600)
}

fn send() -> Event {
    event("chat.message", json!({"text":"work"}))
}

fn turn(session: &Session, method: &str, id: &str) {
    super::super::output::handle_message(
        session,
        json!({"method":method,"params":{"turn":{"id":id}}}),
    );
}

fn accept(session: &Session, events: &[Event]) -> BatchSubmission {
    session.admit_batch(events, || BatchSubmission::Accepted)
}

#[tokio::test]
async fn early_notices_wait_for_the_running_turn_and_land_once_it_completes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (session, _control) = session(temp.path());
    assert!(
        session.ready_for_mail_notice(),
        "an idle Actor takes notices"
    );
    turn(&session, "turn/started", "turn-1");
    assert!(!session.ready_for_mail_notice());
    assert_eq!(accept(&session, &[notice()]), BatchSubmission::Withheld);
    turn(&session, "turn/completed", "turn-1");
    assert_eq!(accept(&session, &[notice()]), BatchSubmission::Accepted);
}

#[tokio::test]
async fn only_early_notices_are_held_and_never_with_real_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (session, _control) = session(temp.path());
    session.set_status("waiting", None);
    assert_eq!(accept(&session, &[notice()]), BatchSubmission::Withheld);
    // A notice sent at the busy delay carries no hold window.
    let busy_notice = event("system.notify", json!({"kind":"mail_notice","context":{}}));
    assert_eq!(accept(&session, &[busy_notice]), BatchSubmission::Accepted);
    // A mixed batch carries real messages the sender wants delivered now.
    assert_eq!(
        accept(&session, &[send(), notice()]),
        BatchSubmission::Accepted
    );
    // Once the busy delay has passed a notice goes out as it always did.
    assert_eq!(
        accept(&session, &[notice_due_in(-1)]),
        BatchSubmission::Accepted
    );
}

use super::*;
use crate::test_support::{spec, test_guard};
use cccc_contracts::{ActorRuntime, ActorSubmit};
use std::time::{Duration, Instant};

fn launch(temp: &tempfile::TempDir, script: &str) -> SessionStatus {
    let mut launch_spec = spec(temp, "g_startup_input", "member", script);
    launch_spec.runner = cccc_contracts::RunnerKind::Pty;
    start_actor_with_history(
        launch_spec,
        HistoryConfig {
            path: temp.path().join("terminal.pty"),
            max_bytes: 1024 * 1024,
            hot_bytes: 8192,
            persist: true,
        },
        ActorLaunch {
            runtime: ActorRuntime::Copilot,
            submit: ActorSubmit::Newline,
        },
    )
    .expect("start fake terminal")
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "fake terminal did not reach its checkpoint"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn submit(expected: &SessionStatus, text: &str) -> PtyHandoff {
    submit_sequence_for_session_with_evidence_interruptible(
        expected,
        text.as_bytes(),
        &[b"\n"],
        Duration::ZERO,
        Duration::ZERO,
        &AtomicBool::new(false),
    )
}

#[test]
fn queued_input_writes_nothing_until_the_native_prompt_is_resolved() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let expected = launch(
        &temp,
        "stty -echo; while [ ! -f paint ]; do sleep 0.01; done; printf 'Confirm folder trust\\r\\n❯ 1. Yes\\r\\n'; touch painted; IFS= read -r choice; printf '%s' \"$choice\" > choice; printf '\\033[2J\\033[H──────────\\r\\n❯\\r\\n──────────\\r\\n'; IFS= read -r first; printf '%s' \"$first\" > first; printf '\\033[2J\\033[HWorking'; IFS= read -r second; printf '%s' \"$second\" > second; sleep 30",
    );
    assert!(expected.startup_input_pending);
    assert!(matches!(
        submit(&expected, "must wait"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    std::fs::write(temp.path().join("paint"), b"").expect("paint");
    wait_until(|| temp.path().join("painted").exists());
    assert!(matches!(
        submit(&expected, "must still wait"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    assert!(
        !temp.path().join("choice").exists(),
        "no business byte may answer the native prompt"
    );
    write(&expected.group_id, &expected.actor_id, b"a\n").expect("manual native choice");
    wait_until(|| match submit(&expected, "first") {
        PtyHandoff::Accepted => true,
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending) => false,
        other => panic!("unexpected handoff: {other:?}"),
    });
    wait_until(|| temp.path().join("first").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("choice")).expect("choice"),
        "a"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("first")).expect("first"),
        "first"
    );
    assert!(
        !status(&expected.group_id, &expected.actor_id)
            .expect("ready status")
            .startup_input_pending
    );
    // The first admission is a Session fact. A later daemon worker's stale
    // snapshot does not require a new input signature while output is in flight.
    assert!(matches!(submit(&expected, "second"), PtyHandoff::Accepted));
    wait_until(|| temp.path().join("second").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("second")).expect("second"),
        "second"
    );
    stop(&expected.group_id, &expected.actor_id).expect("stop");
}

#[test]
fn an_explicit_release_cannot_release_a_replacement_terminal() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let old = launch(&temp, "sleep 30");
    stop(&old.group_id, &old.actor_id).expect("stop old");
    let new = launch(
        &temp,
        "stty -echo; IFS= read -r line; printf '%s' \"$line\" > received; sleep 30",
    );
    assert_ne!(old.started_at, new.started_at);
    assert!(
        !release_startup_input(&new.group_id, &new.actor_id, &old.started_at)
            .expect("stale release")
    );
    assert!(matches!(
        submit(&new, "held"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    assert!(release_startup_input(&new.group_id, &new.actor_id, &new.started_at).expect("release"));
    assert!(
        release_startup_input(&new.group_id, &new.actor_id, &new.started_at)
            .expect("idempotent release")
    );
    assert!(matches!(submit(&new, "one"), PtyHandoff::Accepted));
    wait_until(|| std::fs::read(temp.path().join("received")).is_ok_and(|data| !data.is_empty()));
    assert_eq!(
        std::fs::read_to_string(temp.path().join("received")).expect("received"),
        "one"
    );
    stop(&new.group_id, &new.actor_id).expect("stop new");
}

#[test]
fn new_kilo_viewer_keeps_input_unwritten_until_its_native_editor_is_drawn() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let script = r#"
        stty -echo
        wait_for_file() { while [ ! -f "$1" ]; do sleep 0.01; done; }
        printf '\033[?2004h'
        wait_for_file paint
        printf '  ┃\r\n  ┃\r\n  ┃\r\n  ┃  Ask · Model\r\n  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\033[2;6H'
        wait_for_file disable
        printf '\033[?2004l'
        wait_for_file enable
        printf '\033[?2004h'
        IFS= read -r input
        printf '%s' "$input" > received
        sleep 30
    "#;
    let mut viewer = spec(&temp, "g_new_kilo", "member", script);
    viewer.runner = cccc_contracts::RunnerKind::Pty;
    let actor = start(viewer).expect("viewer");
    let attempt = |text: &[u8]| {
        submit_sequence_for_new_kilo_viewer(
            &actor,
            text,
            &[b"\n"],
            Duration::ZERO,
            Duration::ZERO,
            &AtomicBool::new(false),
        )
    };
    wait_until(|| {
        crate::bracketed_paste_enabled(&actor.group_id, &actor.actor_id).expect("paste mode")
    });
    let wait = |target: &SessionStatus, timeout| {
        wait_for_new_kilo_composer(target, timeout, &AtomicBool::new(false))
            .expect("viewer readiness")
    };
    assert!(!wait(&actor, Duration::ZERO));
    assert!(matches!(
        attempt(b"too soon"),
        PtyHandoff::NotWritten(PtyHandoffReason::KiloComposerPending)
    ));
    std::fs::write(temp.path().join("paint"), b"").expect("paint");
    assert!(wait(&actor, Duration::from_secs(3)));
    let mut previous = actor.clone();
    previous.started_at.push_str("-old");
    assert!(!wait(&previous, Duration::ZERO));
    std::fs::write(temp.path().join("disable"), b"").expect("disable native paste");
    wait_until(|| {
        !crate::bracketed_paste_enabled(&actor.group_id, &actor.actor_id).expect("paste mode")
    });
    assert!(matches!(
        attempt(b"paste was disabled"),
        PtyHandoff::NotWritten(PtyHandoffReason::KiloComposerPending)
    ));
    std::fs::write(temp.path().join("enable"), b"").expect("enable native paste");
    wait_until(|| match attempt(b"first") {
        PtyHandoff::Accepted => true,
        PtyHandoff::NotWritten(PtyHandoffReason::KiloComposerPending) => false,
        other => panic!("unexpected handoff: {other:?}"),
    });
    wait_until(|| std::fs::read(temp.path().join("received")).is_ok_and(|data| !data.is_empty()));
    assert_eq!(
        std::fs::read(temp.path().join("received")).expect("input"),
        b"first"
    );
    stop(&actor.group_id, &actor.actor_id).expect("stop viewer");
}

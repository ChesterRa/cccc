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

fn launch_antigravity(
    temp: &tempfile::TempDir,
    actor_home: &std::path::Path,
    script: &str,
) -> SessionStatus {
    let mut launch_spec = spec(temp, "g_antigravity_onboarding", "agy", script);
    launch_spec.runner = cccc_contracts::RunnerKind::Pty;
    launch_spec
        .env
        .insert("HOME".into(), actor_home.to_string_lossy().into_owned());
    launch_spec.env.insert(
        "USERPROFILE".into(),
        actor_home.to_string_lossy().into_owned(),
    );
    start_actor_with_history(
        launch_spec,
        HistoryConfig {
            path: temp.path().join("agy-terminal.pty"),
            max_bytes: 1024 * 1024,
            hot_bytes: 8192,
            persist: true,
        },
        ActorLaunch {
            runtime: ActorRuntime::Antigravity,
            submit: ActorSubmit::Enter,
        },
    )
    .expect("start fake antigravity terminal")
}

#[test]
fn antigravity_fresh_home_holds_across_wizard_trust_and_partial_redraw_until_bordered_input() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_home = temp.path().join("fresh-home");
    std::fs::create_dir_all(&actor_home).expect("fresh home");
    let onboarding = actor_home.join(".gemini/antigravity-cli/cache/onboarding.json");
    let expected = launch_antigravity(
        &temp,
        &actor_home,
        r#"
        stty -echo
        wait_for_file() { while [ ! -f "$1" ]; do sleep 0.01; done; }
        printf 'Welcome to Antigravity CLI\r\nChoose your color scheme\r\n[Next]\r\n'
        touch wizard
        wait_for_file show_trust
        printf '\033[2J\033[HTrust this workspace?\r\n> 1. Yes, trust\r\n2. No, exit\r\n'
        touch trust
        wait_for_file show_partial
        printf '\033[2J\033[HAntigravity CLI 1.5.16\r\n[Done]\r\n──────────\r\n>\r\n'
        touch partial
        wait_for_file show_prompt
        printf '\033[2J\033[HAntigravity CLI 1.5.16\r\n──────────\r\n>\r\n──────────\r\n? for shortcuts\r\n'
        touch prompt
        while IFS= read -r line; do
            printf '%s\n' "$line" >> received
        done
        sleep 30
        "#,
    );
    assert!(
        expected.startup_input_pending,
        "fresh HOME without onboarding.json must hold automatic input"
    );
    assert!(
        !onboarding.exists(),
        "CCCC must never auto-create onboarding.json"
    );
    wait_until(|| temp.path().join("wizard").exists());
    assert!(matches!(
        submit(&expected, "must wait during wizard"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));

    // Cross-shell onboarding completes, but THIS PTY moves to a trust-only screen.
    std::fs::create_dir_all(onboarding.parent().expect("cache dir")).expect("cache dir");
    std::fs::write(&onboarding, r#"{"onboardingComplete": true}"#).expect("write onboarding");
    assert!(
        status(&expected.group_id, &expected.actor_id)
            .expect("still gated on wizard")
            .startup_input_pending
    );
    assert!(matches!(
        submit(&expected, "still on wizard"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));

    std::fs::write(temp.path().join("show_trust"), b"").expect("show trust");
    wait_until(|| temp.path().join("trust").exists());
    assert!(
        status(&expected.group_id, &expected.actor_id)
            .expect("still gated on trust")
            .startup_input_pending,
        "trust-only screen must hold automatic input even after onboardingComplete=true"
    );
    assert!(matches!(
        submit(&expected, "must not answer trust"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));

    // Banner / buttons / incomplete border redraw must also keep input held.
    std::fs::write(temp.path().join("show_partial"), b"").expect("show partial");
    wait_until(|| temp.path().join("partial").exists());
    assert!(
        status(&expected.group_id, &expected.actor_id)
            .expect("still gated on partial redraw")
            .startup_input_pending,
        "banner/button/partial redraw must hold automatic input"
    );
    assert!(matches!(
        submit(&expected, "must not write into partial redraw"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    assert!(
        !temp.path().join("received").exists(),
        "zero business bytes may be written across wizard, trust, or partial redraw"
    );

    // Only the real bordered chat input box releases automatic delivery, once.
    std::fs::write(temp.path().join("show_prompt"), b"").expect("show prompt");
    wait_until(|| temp.path().join("prompt").exists());
    wait_until(|| match submit(&expected, "first-real-task") {
        PtyHandoff::Accepted => true,
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending) => false,
        other => panic!("unexpected handoff: {other:?}"),
    });
    wait_until(|| temp.path().join("received").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("received")).expect("received"),
        "first-real-task\n"
    );
    assert!(
        !status(&expected.group_id, &expected.actor_id)
            .expect("ready status")
            .startup_input_pending
    );
    stop(&expected.group_id, &expected.actor_id).expect("stop");
}

#[test]
fn antigravity_gate_decision_is_locked_before_spawn_against_concurrent_onboarding() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_home = temp.path().join("race-home");
    std::fs::create_dir_all(&actor_home).expect("race home");
    let onboarding = actor_home.join(".gemini/antigravity-cli/cache/onboarding.json");
    let mut launch_spec = spec(
        &temp,
        "g_antigravity_race",
        "agy",
        r#"
        stty -echo
        printf 'Welcome to Antigravity CLI\r\nTerms of Service & Data Use\r\n[Done]\r\n'
        touch wizard
        while [ ! -f show_prompt ]; do sleep 0.01; done
        printf '\033[2J\033[HAntigravity CLI 1.5.16\r\n──────────\r\n>\r\n──────────\r\n'
        touch prompt
        IFS= read -r first
        printf '%s' "$first" > first
        sleep 30
        "#,
    );
    launch_spec.runner = cccc_contracts::RunnerKind::Pty;
    launch_spec
        .env
        .insert("HOME".into(), actor_home.to_string_lossy().into_owned());
    launch_spec.env.insert(
        "USERPROFILE".into(),
        actor_home.to_string_lossy().into_owned(),
    );
    let onboarding_for_hook = onboarding.clone();
    let expected = start_actor_with_history_and_post_spawn_hook(
        launch_spec,
        HistoryConfig {
            path: temp.path().join("race-terminal.pty"),
            max_bytes: 1024 * 1024,
            hot_bytes: 8192,
            persist: true,
        },
        ActorLaunch {
            runtime: ActorRuntime::Antigravity,
            submit: ActorSubmit::Enter,
        },
        move || {
            // Simulate another shell finishing onboarding between spawn_pty and session registration.
            std::fs::create_dir_all(onboarding_for_hook.parent().expect("cache dir"))
                .expect("cache dir");
            std::fs::write(&onboarding_for_hook, r#"{"onboardingComplete": true}"#)
                .expect("concurrent onboarding write");
        },
    )
    .expect("start race terminal");
    assert!(
        expected.startup_input_pending,
        "gate locked before spawn must not be bypassed by post-spawn onboarding completion"
    );
    wait_until(|| temp.path().join("wizard").exists());
    assert!(matches!(
        submit(&expected, "must stay held"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    assert!(!temp.path().join("first").exists());
    std::fs::write(temp.path().join("show_prompt"), b"").expect("show prompt");
    wait_until(|| temp.path().join("prompt").exists());
    wait_until(|| match submit(&expected, "delivered-after-wizard") {
        PtyHandoff::Accepted => true,
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending) => false,
        other => panic!("unexpected handoff: {other:?}"),
    });
    wait_until(|| temp.path().join("first").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("first")).expect("first"),
        "delivered-after-wizard"
    );
    stop(&expected.group_id, &expected.actor_id).expect("stop");
}

#[test]
fn antigravity_onboarded_home_delivers_without_terminal_label_gating() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_home = temp.path().join("onboarded-home");
    let onboarding = actor_home.join(".gemini/antigravity-cli/cache/onboarding.json");
    std::fs::create_dir_all(onboarding.parent().expect("cache dir")).expect("cache dir");
    std::fs::write(&onboarding, r#"{"onboardingComplete": true}"#).expect("write onboarding");
    let expected = launch_antigravity(
        &temp,
        &actor_home,
        "stty -echo; IFS= read -r first; printf '%s' \"$first\" > first; sleep 30",
    );
    assert!(
        !expected.startup_input_pending,
        "already-onboarded HOME must not gate on terminal labels"
    );
    assert!(matches!(
        submit(&expected, "immediate"),
        PtyHandoff::Accepted
    ));
    wait_until(|| temp.path().join("first").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("first")).expect("first"),
        "immediate"
    );
    stop(&expected.group_id, &expected.actor_id).expect("stop");
}

#[test]
fn antigravity_fresh_home_supports_explicit_user_release_with_generation_check() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_home = temp.path().join("explicit-home");
    std::fs::create_dir_all(&actor_home).expect("explicit home");
    let expected = launch_antigravity(
        &temp,
        &actor_home,
        "stty -echo; IFS= read -r first; printf '%s' \"$first\" > first; sleep 30",
    );
    assert!(expected.startup_input_pending);
    assert!(
        !release_startup_input(&expected.group_id, &expected.actor_id, "stale-generation")
            .expect("stale release")
    );
    assert!(matches!(
        submit(&expected, "held"),
        PtyHandoff::NotWritten(PtyHandoffReason::StartupInputPending)
    ));
    assert!(
        release_startup_input(&expected.group_id, &expected.actor_id, &expected.started_at)
            .expect("explicit release")
    );
    assert!(matches!(
        submit(&expected, "released"),
        PtyHandoff::Accepted
    ));
    wait_until(|| temp.path().join("first").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("first")).expect("first"),
        "released"
    );
    stop(&expected.group_id, &expected.actor_id).expect("stop");
}

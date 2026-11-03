//! Real lifecycle and PTY input, with the existing offline provider as the boundary.
use super::{Fixture, SAVED, isolated};
use crate::ops::actor_delivery_worker::{BatchOutcome, process_batch};
use crate::ops::local_headless as headless;
use serde_json::Value;
use std::sync::atomic::AtomicBool;

#[path = "claude_confirmation_options_tests.rs"]
mod options;
#[path = "claude_confirmation_terminal_tests.rs"]
mod terminal;

fn start_prompt(f: &Fixture) {
    let started = f.lifecycle("actor_start");
    assert!(started.ok, "{:?}", started.error);
    f.wait(|| f.config.join("prompt_stage").exists());
    f.assert_no_private_output(&started);
    assert!(
        headless::waiting_for_workspace_trust(&f.group.group_id, &f.actor.id)
            .expect("check confirmation watcher")
    );
    assert!(!headless::running(&f.group.group_id, &f.actor.id));
    let launch: Value = cccc_core::fs::read_json(&f.config.join("prompt_launch.json"))
        .expect("read confirmation launch");
    assert!(
        launch["argv"]
            .as_array()
            .expect("confirmation arguments")
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions")
    );
    assert!(
        !launch["argv"]
            .as_array()
            .expect("confirmation arguments")
            .iter()
            .any(|arg| arg == "--bg" || arg == "agents")
    );
    assert_eq!(
        launch["config_dir"],
        f.config
            .canonicalize()
            .expect("canonical fixture config")
            .to_string_lossy()
            .as_ref()
    );
    f.assert_launch_contexts_match();
}

fn input(f: &Fixture, bytes: &[u8]) {
    cccc_runtime::write(&f.group.group_id, &f.actor.id, bytes).expect("human input to fake PTY");
}

fn waiting(f: &Fixture) -> bool {
    headless::waiting_for_workspace_trust(&f.group.group_id, &f.actor.id)
        .expect("check confirmation watcher")
}

fn assert_saved_receipt(f: &Fixture, failures: u64, status: &str) {
    let receipt = f.receipt();
    assert_eq!(receipt["provider_session_id"], SAVED);
    assert_eq!(receipt["failure_count"], failures);
    assert_eq!(receipt["status"], status);
}

#[test]
fn accepting_disclaimer_in_pty_resumes_and_delivers_only_after_confirmation() {
    if isolated(
        "confirmation::accepting_disclaimer_in_pty_resumes_and_delivers_only_after_confirmation",
    ) {
        return;
    }
    let f = Fixture::new("disclaimer");
    let old_global = std::fs::read(f.config.join(".claude.json")).ok();
    start_prompt(&f);
    let prompt_pid = cccc_runtime::status(&f.group.group_id, &f.actor.id)
        .expect("confirmation terminal status")
        .pid;
    let job = f.delivery();
    let event = job.event.clone();
    assert_eq!(
        process_batch(&mut vec![job], &mut String::new(), &AtomicBool::new(false)),
        BatchOutcome::Retry
    );
    assert!(!f.config.join("received").exists());
    assert!(!f.config.join("confirmation_inputs").exists());
    assert!(!f.config.join("settings.json").exists());
    assert_saved_receipt(&f, 0, "usable");
    let queued = crate::ops::actor_delivery::dispatch(&f.home, &f.group, &event);
    assert_eq!(queued.queued, 1);
    f.wait(|| f.delivery_state(&event.id).as_deref() == Some("claimed"));
    assert!(waiting(&f));
    assert!(!f.config.join("received").exists());
    assert!(!f.config.join("confirmation_inputs").exists());

    input(&f, b"yes\r");
    f.wait(|| {
        std::fs::read_to_string(f.config.join("received"))
            .is_ok_and(|text| text.contains("PENDING_TASK"))
    });
    f.wait(|| f.delivery_state(&event.id).as_deref() == Some("accepted"));
    f.wait(|| !waiting(&f));
    assert_eq!(f.launches(), 2);
    assert!(headless::running(&f.group.group_id, &f.actor.id));
    assert_ne!(
        cccc_runtime::status(&f.group.group_id, &f.actor.id)
            .expect("managed terminal status")
            .pid,
        prompt_pid
    );
    assert_saved_receipt(&f, 0, "usable");
    assert_eq!(
        std::fs::read(f.config.join(".claude.json")).ok(),
        old_global
    );
    let settings: Value = cccc_core::fs::read_json(&f.config.join("settings.json"))
        .expect("read approved fixture settings");
    assert_eq!(settings["skipDangerousModePermissionPrompt"], true);
    assert_eq!(
        std::fs::read_to_string(f.config.join("confirmation_inputs"))
            .expect("read fixture confirmation input"),
        "\"yes\"\n"
    );
    let received = std::fs::read_to_string(f.config.join("received")).expect("read delivery");
    assert_eq!(received.matches("PENDING_TASK").count(), 1);
    assert!(!f.config.join("unexpected_confirmation_tasks").exists());
    f.assert_launch_contexts_match();
}

#[test]
fn default_home_approval_preserves_an_unset_config_directory() {
    if super::isolated_with_config_dir(
        "confirmation::default_home_approval_preserves_an_unset_config_directory",
        false,
    ) {
        return;
    }
    for fresh in [false, true] {
        let f = Fixture::with_config_dir("disclaimer", false);
        if fresh {
            std::fs::remove_file(f.receipt_path()).expect("remove saved fixture receipt");
        }
        start_prompt(&f);
        let prompt: Value = cccc_core::fs::read_json(&f.config.join("prompt_launch.json"))
            .expect("read confirmation launch");
        assert!(prompt["config_dir_env"].is_null());
        assert_eq!(prompt["home"], f.actor.env["HOME"]);
        input(&f, b"yes\r");
        f.wait(|| headless::running(&f.group.group_id, &f.actor.id) && !waiting(&f));
        assert_eq!(f.launches(), 2);
        if fresh {
            assert_ne!(f.receipt()["provider_session_id"], SAVED);
            for line in std::fs::read_to_string(f.config.join("launch_contexts"))
                .expect("read fixture launch contexts")
                .lines()
            {
                f.assert_private_settings(
                    &serde_json::from_str(line).expect("parse launch context"),
                );
            }
        } else {
            assert_saved_receipt(&f, 0, "usable");
        }
        f.assert_launch_contexts_match();
    }
}

#[test]
fn declining_or_closing_the_disclaimer_does_not_start_a_managed_session() {
    if isolated(
        "confirmation::declining_or_closing_the_disclaimer_does_not_start_a_managed_session",
    ) {
        return;
    }
    for answer in [b"no\r".as_slice(), b"\x03".as_slice()] {
        let f = Fixture::new("disclaimer");
        start_prompt(&f);
        input(&f, answer);
        f.wait(|| !waiting(&f));
        assert!(
            !cccc_runtime::status(&f.group.group_id, &f.actor.id)
                .is_ok_and(|status| status.running)
        );
        assert!(!headless::running(&f.group.group_id, &f.actor.id));
        assert_eq!(f.launches(), 1);
        assert!(!f.config.join("settings.json").exists());
        assert!(!f.config.join("received").exists());
        assert_saved_receipt(&f, 0, "usable");
    }
}

#[test]
fn declined_confirmation_keeps_claimed_work_for_one_explicit_retry() {
    if isolated("confirmation::declined_confirmation_keeps_claimed_work_for_one_explicit_retry") {
        return;
    }
    let f = Fixture::new("disclaimer");
    start_prompt(&f);
    input(&f, b"no\r");
    f.wait(|| !waiting(&f));
    assert!(
        headless::confirmation_paused(&f.group.group_id, &f.actor.id)
            .expect("check declined confirmation")
    );

    let job = f.delivery();
    let event_id = job.event.id.clone();
    assert_eq!(
        process_batch(&mut vec![job], &mut String::new(), &AtomicBool::new(false)),
        BatchOutcome::Delivered
    );
    assert!(!f.config.join("received").exists());
    assert_eq!(f.delivery_state(&event_id).as_deref(), Some("failed"));

    // The first prompt left this marker behind. Waiting only for the watcher
    // would allow input before the replacement fixture is ready to read it.
    std::fs::remove_file(f.config.join("prompt_stage")).expect("clear old prompt marker");
    let retried = f.lifecycle("actor_start");
    assert!(retried.ok, "{:?}", retried.error);
    f.wait(|| waiting(&f) && f.config.join("prompt_stage").exists());
    assert!(!f.config.join("received").exists());
    input(&f, b"yes\r");
    f.wait(|| f.delivery_state(&event_id).as_deref() == Some("accepted"));
    let received = std::fs::read_to_string(f.config.join("received")).expect("fixture receipt");
    assert_eq!(received.matches("PENDING_TASK").count(), 1);
    assert!(!f.config.join("unexpected_confirmation_tasks").exists());
}

#[test]
fn actor_stop_retires_the_confirmation_owner_without_accepting() {
    if isolated("confirmation::actor_stop_retires_the_confirmation_owner_without_accepting") {
        return;
    }
    let f = Fixture::new("disclaimer");
    start_prompt(&f);
    let stopped = f.lifecycle("actor_stop");
    assert!(stopped.ok, "{:?}", stopped.error);
    f.wait(|| !waiting(&f));
    assert!(cccc_runtime::write(&f.group.group_id, &f.actor.id, b"yes\r").is_err());
    assert!(!headless::running(&f.group.group_id, &f.actor.id));
    assert!(!f.config.join("settings.json").exists());
    assert!(!f.config.join("active").exists());
    assert_eq!(f.launches(), 1);
    assert_saved_receipt(&f, 0, "usable");
}

#[test]
fn workspace_and_disclaimer_require_two_human_confirmations() {
    if isolated("confirmation::workspace_and_disclaimer_require_two_human_confirmations") {
        return;
    }
    let f = Fixture::new("workspace_then_disclaimer");
    start_prompt(&f);
    let prompt_pid = cccc_runtime::status(&f.group.group_id, &f.actor.id)
        .expect("confirmation terminal status")
        .pid;
    assert_eq!(
        std::fs::read_to_string(f.config.join("prompt_stage")).expect("workspace prompt stage"),
        "workspace"
    );
    input(&f, b"yes\r");
    f.wait(|| f.launches() == 2);
    assert_eq!(
        std::fs::read_to_string(f.config.join("prompt_stage")).expect("disclaimer prompt stage"),
        "disclaimer"
    );
    assert!(waiting(&f));
    assert!(!headless::running(&f.group.group_id, &f.actor.id));
    assert!(!f.config.join("settings.json").exists());
    assert_eq!(
        cccc_runtime::status(&f.group.group_id, &f.actor.id)
            .expect("confirmation terminal status")
            .pid,
        prompt_pid
    );
    assert_saved_receipt(&f, 0, "usable");

    input(&f, b"yes\r");
    f.wait(|| headless::running(&f.group.group_id, &f.actor.id) && !waiting(&f));
    assert_eq!(f.launches(), 3);
    assert_saved_receipt(&f, 0, "usable");
    assert_eq!(
        std::fs::read_to_string(f.config.join("confirmation_inputs"))
            .expect("read fixture confirmation inputs"),
        "\"yes\"\n\"yes\"\n"
    );
    assert!(!f.config.join("unexpected_confirmation_tasks").exists());
}

#[test]
fn a_settings_change_is_not_approval_and_an_ordinary_failure_is_not_success() {
    if isolated(
        "confirmation::a_settings_change_is_not_approval_and_an_ordinary_failure_is_not_success",
    ) {
        return;
    }
    let f = Fixture::new("confirmation_failure");
    start_prompt(&f);
    let prompt_pid = cccc_runtime::status(&f.group.group_id, &f.actor.id)
        .expect("confirmation terminal status")
        .pid;
    // This is a harmless edit to the isolated fixture, not provider consent.
    std::fs::write(f.config.join("settings.json"), r#"{"theme":"dark"}"#)
        .expect("write unrelated fixture setting");
    f.wait(|| f.launches() == 2);
    assert!(waiting(&f));
    assert_eq!(
        cccc_runtime::status(&f.group.group_id, &f.actor.id)
            .expect("confirmation terminal status")
            .pid,
        prompt_pid
    );
    assert!(!f.config.join("active").exists());
    assert!(!f.config.join("confirmation_inputs").exists());
    assert_saved_receipt(&f, 0, "usable");

    input(&f, b"yes\r");
    f.wait(|| !waiting(&f));
    assert!(
        !cccc_runtime::status(&f.group.group_id, &f.actor.id).is_ok_and(|status| status.running)
    );
    assert!(!headless::running(&f.group.group_id, &f.actor.id));
    assert_eq!(f.launches(), 3);
    assert!(!f.config.join("active").exists());
    assert!(!f.config.join("received").exists());
    assert_saved_receipt(&f, 1, "resume_failed");
    let failed = f.lifecycle("actor_start");
    assert!(!failed.ok);
    f.assert_no_private_output(&failed);
}

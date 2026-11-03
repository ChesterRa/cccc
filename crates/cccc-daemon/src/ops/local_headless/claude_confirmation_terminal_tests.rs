use super::{Fixture, isolated, start_prompt, waiting};
use crate::ops::actor_delivery_worker::{BatchOutcome, process_batch};
use crate::ops::local_headless as headless;
use std::sync::atomic::AtomicBool;

#[test]
fn cancelled_watcher_cannot_reclassify_its_live_confirmation_terminal() {
    if isolated(
        "confirmation::terminal::cancelled_watcher_cannot_reclassify_its_live_confirmation_terminal",
    ) {
        return;
    }
    let f = Fixture::new("disclaimer");
    start_prompt(&f);
    let original = cccc_runtime::status(&f.group.group_id, &f.actor.id).expect("original prompt");
    // Reproduce cancellation followed by failed preparation: the old PTY is still alive.
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).expect("cancel watcher");
    f.wait(|| !waiting(&f));
    let retained = cccc_runtime::status(&f.group.group_id, &f.actor.id).expect("retained prompt");
    assert!(retained.running);
    assert_eq!(
        (retained.pid, retained.started_at),
        (original.pid, original.started_at)
    );
    let mut jobs = vec![f.delivery()];
    let id = jobs[0].event.id.clone();
    for _ in 0..2 {
        assert_eq!(
            process_batch(&mut jobs, &mut String::new(), &AtomicBool::new(false)),
            BatchOutcome::Retry
        );
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].event.id, id);
        assert!(jobs[0].claimed);
    }
    assert_eq!(f.launches(), 1);
    assert!(!f.config.join("confirmation_inputs").exists());
    assert!(!f.config.join("unexpected_confirmation_tasks").exists());
    assert!(!f.config.join("received").exists());

    let stopped = f.lifecycle("actor_stop");
    assert!(stopped.ok, "{:?}", stopped.error);
    assert!(
        !headless::confirmation_terminal::owns(&f.group.group_id, &f.actor.id)
            .expect("check retired ownership")
    );
    let restarted = f.lifecycle("actor_start");
    assert!(restarted.ok, "{:?}", restarted.error);
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).expect("cancel second watcher");
    f.wait(|| !waiting(&f));
    cccc_runtime::stop(&f.group.group_id, &f.actor.id).expect("close the second confirmation PTY");
    // Replace before querying protection: the stale identity must not cover this new PTY.
    let ordinary = cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: f.group.group_id.clone(),
        actor_id: f.actor.id.clone(),
        runner: cccc_contracts::RunnerKind::Pty,
        command: vec!["/bin/cat".into()],
        cwd: f._temp.path().to_owned(),
        env: f.actor.env.clone(),
        cols: 80,
        rows: 24,
    })
    .expect("ordinary replacement PTY");
    assert!(ordinary.running);
    // Group cleanup may still find the stale protected identity, but it must
    // never stop the newer terminal that now occupies the same Actor key.
    headless::stop_group(&f.group.group_id).expect("retire stale confirmation owner");
    assert!(
        cccc_runtime::status(&f.group.group_id, &f.actor.id)
            .is_ok_and(|status| status.running && status.pid == ordinary.pid)
    );
    let started = f.lifecycle("actor_start");
    assert!(started.ok, "{:?}", started.error);
    assert!(
        cccc_runtime::status(&f.group.group_id, &f.actor.id)
            .is_ok_and(|status| status.running && status.pid == ordinary.pid)
    );
    assert!(!headless::uses_managed_delivery(
        &f.group.group_id,
        &f.actor
    ));
}

#[test]
fn pausing_group_retires_confirmation_even_if_its_watcher_was_cancelled() {
    if isolated(
        "confirmation::terminal::pausing_group_retires_confirmation_even_if_its_watcher_was_cancelled",
    ) {
        return;
    }
    let f = Fixture::new("disclaimer");
    start_prompt(&f);
    // A later start can cancel the watcher before preparation fails, leaving
    // its protected PTY alive without a registered recovery worker.
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).expect("cancel watcher");
    f.wait(|| !waiting(&f));
    assert!(
        headless::confirmation_terminal::owns(&f.group.group_id, &f.actor.id)
            .expect("check protected prompt")
    );
    let paused = crate::handle_request(
        &f.home,
        &cccc_contracts::DaemonRequest {
            v: 1,
            op: "group_set_state".into(),
            args: serde_json::json!({"group_id":f.group.group_id,"by":"user","state":"paused"})
                .as_object()
                .expect("state arguments")
                .clone(),
        },
    );
    assert!(paused.ok, "{:?}", paused.error);
    assert!(
        !cccc_runtime::status(&f.group.group_id, &f.actor.id).is_ok_and(|status| status.running)
    );
    assert!(!f.config.join("confirmation_inputs").exists());
}

#[test]
fn explicit_start_replaces_an_orphaned_confirmation() {
    if isolated("confirmation::terminal::explicit_start_replaces_an_orphaned_confirmation") {
        return;
    }
    let f = Fixture::new("disclaimer");
    start_prompt(&f);
    let original = cccc_runtime::status(&f.group.group_id, &f.actor.id)
        .expect("original confirmation terminal");
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).expect("cancel watcher");
    assert!(
        !headless::confirmation_pending_active(&f.group.group_id, &f.actor.id)
            .expect("check cancelled watcher")
    );
    assert!(
        headless::confirmation_terminal::owns(&f.group.group_id, &f.actor.id)
            .expect("check orphaned prompt")
    );
    let event = f.delivery().event;
    assert_eq!(
        crate::ops::actor_delivery::dispatch(&f.home, &f.group, &event).queued,
        1
    );
    f.wait(|| f.delivery_state(&event.id).as_deref() == Some("claimed"));
    std::fs::remove_file(f.config.join("prompt_stage")).expect("clear old prompt marker");
    let started = f.lifecycle("actor_start");
    assert!(started.ok, "{:?}", started.error);
    assert!(
        waiting(&f),
        "explicit Start must restore a confirmation watcher"
    );
    let replacement = cccc_runtime::status(&f.group.group_id, &f.actor.id)
        .expect("replacement confirmation terminal");
    assert_ne!(
        (replacement.pid, replacement.started_at),
        (original.pid, original.started_at)
    );
    f.wait(|| f.config.join("prompt_stage").exists());
    super::input(&f, b"yes\r");
    f.wait(|| f.delivery_state(&event.id).as_deref() == Some("accepted"));
    let received = std::fs::read_to_string(f.config.join("received")).expect("read delivery");
    assert_eq!(received.matches("PENDING_TASK").count(), 1);
}

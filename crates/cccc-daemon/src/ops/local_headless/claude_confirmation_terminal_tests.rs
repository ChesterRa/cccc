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
    let original = cccc_runtime::status(&f.group.group_id, &f.actor.id).unwrap();
    // Reproduce cancellation followed by failed preparation: the old PTY is still alive.
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).unwrap();
    f.wait(|| !waiting(&f));
    let retained = cccc_runtime::status(&f.group.group_id, &f.actor.id).unwrap();
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
    assert!(!headless::confirmation_terminal::owns(&f.group.group_id, &f.actor.id).unwrap());
    let restarted = f.lifecycle("actor_start");
    assert!(restarted.ok, "{:?}", restarted.error);
    headless::retry_confirmation(&f.group.group_id, &f.actor.id).unwrap();
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
    assert!(!headless::uses_managed_delivery(
        &f.group.group_id,
        &f.actor
    ));
}

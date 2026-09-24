use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn fixture() -> (tempfile::TempDir, HomeLayout, Arc<Recovery>) {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let key = (format!("trust-{}", uuid::Uuid::new_v4()), "actor".into());
    let recovery = Arc::new(Recovery::register(key).expect("register prompt"));
    (temp, home, recovery)
}

// Provider launch/attachment/cleanup are the external boundary. The launch guards,
// cancellation registry and supervisor stop paths below are the production ones.
fn stop_during_launch(group_stop: bool) {
    let (_temp, home, recovery) = fixture();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let attached = Arc::new(AtomicBool::new(false));
    let discarded = Arc::new(AtomicBool::new(false));
    let worker_recovery = Arc::clone(&recovery);
    let published = Arc::clone(&attached);
    let cleaned = Arc::clone(&discarded);
    let worker = std::thread::spawn(move || {
        recover(
            &home,
            &worker_recovery,
            || true,
            || {
                entered_tx.send(()).expect("launch entered");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("finish provider launch");
                Ok(())
            },
            |_| {
                published.store(true, Ordering::Release);
                Ok(())
            },
            |_| {
                cleaned.store(true, Ordering::Release);
                Ok(())
            },
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("provider launch started");
    let key = recovery.key.clone();
    let stopper = std::thread::spawn(move || {
        if group_stop {
            super::super::supervisor::stop_group(&key.0).expect("stop group");
        } else {
            super::super::supervisor::stop(&key.0, &key.1).expect("stop actor");
        }
        stopped_tx.send(()).expect("stop complete");
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !recovery.cancelled() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let cancelled_before_completion = recovery.cancelled();
    let stopped_early = stopped_rx.try_recv().is_ok();
    release_tx.send(()).expect("release launch");
    assert!(
        worker
            .join()
            .expect("launch thread")
            .expect("recovery outcome")
    );
    stopper.join().expect("stop thread");
    assert!(
        cancelled_before_completion,
        "stop must cancel an in-flight recovery"
    );
    assert!(!stopped_early, "stop must wait for the attachment boundary");
    assert!(
        !attached.load(Ordering::Acquire),
        "cancelled Actor must not be reattached"
    );
    assert!(
        discarded.load(Ordering::Acquire),
        "new provider session must be cleaned up"
    );
}

#[test]
fn actor_stop_cancels_an_in_flight_trust_launch() {
    stop_during_launch(false);
}

#[test]
fn group_stop_includes_a_prompt_without_an_attached_session() {
    stop_during_launch(true);
}

#[test]
fn waiting_for_runtime_permission_does_not_reserve_the_actor_start_guard() {
    let (_temp, home, recovery) = fixture();
    // Normal Actor startup already owns this permit before taking StartGuard.
    let permit = crate::runtime_start_gate::permit(&home).expect("normal start permit");
    let worker_home = home.clone();
    let worker_recovery = Arc::clone(&recovery);
    let (ready_tx, ready_rx) = mpsc::channel();
    let retry = std::thread::spawn(move || {
        ready_tx.send(()).expect("retry scheduled");
        recover(
            &worker_home,
            &worker_recovery,
            || true,
            || Ok(()),
            |_| Ok(()),
            |_| Ok(()),
        )
    });
    ready_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("retry running");
    // Give the competing retry a chance to block on the held global permit.
    std::thread::sleep(Duration::from_millis(100));
    let key = recovery.key.clone();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let normal = std::thread::spawn(move || {
        let _start = StartGuard::acquire(&key).expect("normal Actor start guard");
        acquired_tx
            .send(())
            .expect("normal Actor start can proceed");
    });
    let progressed = acquired_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    // Release even when the old lock ordering blocked progress, so failures cannot
    // poison the shared runtime gate or hang the rest of the test process.
    drop(permit);
    normal.join().expect("normal start thread");
    retry.join().expect("retry thread").expect("retry result");
    assert!(
        progressed,
        "trust retry must not hold StartGuard while waiting for a permit"
    );
}

#[test]
fn retiring_an_old_prompt_does_not_unregister_its_replacement() {
    let (_temp, _home, old) = fixture();
    let replacement = Recovery::register(old.key.clone()).expect("replacement prompt");
    assert!(old.cancelled());
    drop(old);
    super::super::supervisor::stop(&replacement.key.0, &replacement.key.1)
        .expect("stop replacement");
    assert!(replacement.cancelled());
}

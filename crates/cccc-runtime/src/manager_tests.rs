use super::{
    commit_reaped, start, start_with_history, status, stop, stop_all, stop_if_started_at,
    submit_interruptible, submit_sequence_interruptible, write,
};
use crate::registry::lookup;
use crate::test_support::{spec, test_guard};
use crate::{HistoryConfig, RuntimeError, history, history_since};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[test]
#[cfg(target_os = "linux")]
fn stop_can_finish_while_terminal_input_is_backpressured() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_blocked_terminal_input";
    let actor = "peer1";
    start(spec(
        &temp,
        group,
        actor,
        "stty raw -echo; touch ready; i=0; while [ ! -f release ] && [ $i -lt 500 ]; do sleep 0.02; i=$((i + 1)); done; dd bs=65536 count=16 iflag=fullblock of=/dev/null 2>/dev/null",
    ))
    .expect("non-reading terminal");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !temp.path().join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let ready = temp.path().join("ready").exists();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        entered_tx.send(()).expect("entered");
        write(group, actor, &vec![b'x'; 1024 * 1024])
    });
    entered_rx.recv().expect("writer started");
    // Allow the PTY input queue to fill. The fixture never consumes those bytes.
    std::thread::sleep(Duration::from_millis(100));
    let backpressured = !writer.is_finished();
    let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        let result = stop(group, actor);
        stopped_tx.send(result).expect("stopped receiver");
    });
    let stopped_without_release = stopped_rx.recv_timeout(Duration::from_secs(1));
    // Always release our fixture before assertions, including on the old code,
    // so reproducing the bug does not leave a blocked worker or child behind.
    std::fs::write(temp.path().join("release"), b"").expect("release fixture");
    stopper.join().expect("stopper");
    let _ = writer.join().expect("writer");
    assert!(
        ready && backpressured,
        "fixture must reach blocked input before stop"
    );
    stopped_without_release
        .expect("stop must not wait for the Actor to consume a blocked terminal write")
        .expect("stop");
}

#[test]
#[cfg(target_os = "linux")]
fn cancellation_releases_backpressured_input_and_waiting_submission() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_cancel_blocked_input";
    let actor = "peer1";
    start(spec(
        &temp, group, actor,
        "stty raw -echo; touch ready; i=0; while [ ! -f release ] && [ $i -lt 500 ]; do sleep 0.02; i=$((i + 1)); done; dd bs=65536 count=16 iflag=fullblock of=/dev/null 2>/dev/null",
    )).expect("non-reading terminal");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !temp.path().join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let ready = temp.path().join("ready").exists();
    let blocked_cancel = Arc::new(AtomicBool::new(false));
    let cancel = Arc::clone(&blocked_cancel);
    let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
    let blocked = std::thread::spawn(move || {
        let result = submit_interruptible(
            group,
            actor,
            &vec![b'x'; 1024 * 1024],
            b"",
            Duration::ZERO,
            &cancel,
        );
        blocked_tx.send(result).expect("blocked receiver");
    });
    std::thread::sleep(Duration::from_millis(100));
    let backpressured = !blocked.is_finished();
    let waiting_cancel = Arc::new(AtomicBool::new(false));
    let cancel = Arc::clone(&waiting_cancel);
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let waiting = std::thread::spawn(move || {
        let result = submit_interruptible(
            group,
            actor,
            b"must-not-be-written",
            b"",
            Duration::ZERO,
            &cancel,
        );
        waiting_tx.send(result).expect("waiting receiver");
    });
    std::thread::sleep(Duration::from_millis(100));
    let gate_blocked = !waiting.is_finished();
    waiting_cancel.store(true, Ordering::Release);
    let waiting_result = waiting_rx.recv_timeout(Duration::from_secs(1));
    let first_still_blocked = !blocked.is_finished();
    blocked_cancel.store(true, Ordering::Release);
    let blocked_result = blocked_rx.recv_timeout(Duration::from_secs(1));
    // Release before asserting so the reproducer also cleans up on regression.
    std::fs::write(temp.path().join("release"), b"").expect("release");
    stop(group, actor).expect("cleanup");
    blocked.join().expect("blocked submission");
    waiting.join().expect("waiting submission");
    assert!(ready && backpressured && gate_blocked && first_still_blocked);
    assert!(
        !waiting_result
            .expect("gate wait must be cancellable")
            .expect("waiting result")
    );
    assert!(
        !blocked_result
            .expect("blocked PTY write must be cancellable")
            .expect("blocked result")
    );
}

#[test]
#[cfg(target_os = "linux")]
fn revoked_attachment_releases_backpressured_input() {
    let _guard = test_guard();
    for takeover in [false, true] {
        let temp = tempfile::tempdir().expect("tempdir");
        let group = "g_revoked_blocked_input";
        let actor = "peer1";
        start(spec(
            &temp,
            group,
            actor,
            "stty raw -echo; touch ready; sleep 30",
        ))
        .expect("terminal");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !temp.path().join("ready").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let attachment = crate::attach(
            group,
            actor,
            crate::TerminalAttachMode::Control,
            false,
            None,
        )
        .expect("attachment");
        let input = attachment.input();
        let (tx, rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            tx.send(input.write(&vec![b'x'; 1024 * 1024]))
                .expect("receiver");
        });
        std::thread::sleep(Duration::from_millis(100));
        let blocked = !writer.is_finished();
        let replacement = if takeover {
            Some(
                crate::attach(group, actor, crate::TerminalAttachMode::Control, true, None)
                    .expect("takeover"),
            )
        } else {
            drop(attachment);
            None
        };
        let result = rx.recv_timeout(Duration::from_secs(1));
        stop(group, actor).expect("cleanup");
        writer.join().expect("writer");
        drop(replacement);
        assert!(blocked, "fixture must block before revocation");
        assert!(
            !result
                .expect("revocation must release blocked input")
                .expect("write result")
        );
    }
}

#[test]
fn submission_does_not_continue_in_a_replacement_session() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_submission_generation";
    let actor = "peer1";
    start(spec(
        &temp,
        group,
        actor,
        "stty raw -echo; touch ready; dd bs=1 count=1 of=received 2>/dev/null; sleep 10",
    ))
    .expect("first session");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !temp.path().join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let submitting = std::thread::spawn(move || {
        submit_interruptible(
            group,
            actor,
            b"x",
            b"must-not-reach-replacement",
            Duration::from_secs(2),
            &AtomicBool::new(false),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::fs::metadata(temp.path().join("received")).map_or(0, |m| m.len()) != 1
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    stop(group, actor).expect("stop first");
    start(spec(
        &temp,
        group,
        actor,
        "stty raw -echo; cat > replacement-input",
    ))
    .expect("replacement");
    let received = std::fs::read(temp.path().join("received")).expect("received first payload");
    let result = submitting.join().expect("submission thread");
    stop(group, actor).expect("cleanup replacement");
    assert_eq!(received, b"x");
    assert!(
        result.is_err(),
        "a stopped generation cannot accept the submit suffix"
    );
    assert!(
        std::fs::read(temp.path().join("replacement-input"))
            .unwrap_or_default()
            .is_empty()
    );
}

#[test]
fn cold_terminal_readiness_preserves_large_input() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_cold_input";
    let actor = "peer1";
    let payload = format!("{}TAIL_MARKER", "x".repeat(16_038));
    start(spec(
        &temp, group, actor,
        &format!("sleep 0.2; stty raw -echo; printf '\\033[?2004h'; dd bs=1 count={} of=received 2>/dev/null; sleep 2", payload.len()),
    )).expect("cold terminal");
    let cancelled = AtomicBool::new(false);
    assert!(
        super::wait_for_input_ready(group, actor, Duration::from_secs(3), &cancelled)
            .expect("input readiness")
    );
    let submitted = submit_sequence_interruptible(
        group,
        actor,
        payload.as_bytes(),
        &[],
        Duration::ZERO,
        Duration::ZERO,
        &cancelled,
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::fs::metadata(temp.path().join("received")).map_or(0, |m| m.len())
        < payload.len() as u64
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    stop(group, actor).expect("stop");
    assert!(submitted.expect("submit"));
    assert_eq!(
        std::fs::read(temp.path().join("received")).expect("received"),
        payload.as_bytes()
    );
}

#[test]
fn unready_terminal_timeout_and_cancel_do_not_write_input() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_unready_input";
    let actor = "peer1";
    start(spec(&temp, group, actor, "sleep 5")).expect("terminal");
    assert!(
        !super::wait_for_input_ready(
            group,
            actor,
            Duration::from_millis(50),
            &AtomicBool::new(false)
        )
        .expect("timeout")
    );
    assert!(
        !super::wait_for_input_ready(group, actor, Duration::from_secs(5), &AtomicBool::new(true))
            .expect("cancel")
    );
    assert!(
        !history(group, actor, None, 1024)
            .expect("history")
            .data
            .contains("bootstrap")
    );
    stop(group, actor).expect("stop");
}

#[test]
fn captures_process_output() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(
        &temp,
        "g_test",
        "peer1",
        "printf runtime-ready; sleep 1",
    ))
    .expect("start");
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        history("g_test", "peer1", None, 1024)
            .expect("history")
            .data
            .contains("runtime-ready")
    );
    assert!(status("g_test", "peer1").expect("status").running);
    stop("g_test", "peer1").expect("stop");
}

#[test]
fn stop_all_terminates_every_runtime() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    for actor in ["peer1", "peer2"] {
        start(spec(&temp, "g_stop_all", actor, "sleep 30")).expect("start");
    }
    assert_eq!(stop_all().expect("stop all").len(), 2);
    assert!(status("g_stop_all", "peer1").is_err());
    assert!(status("g_stop_all", "peer2").is_err());
}

#[test]
fn conditional_stop_preserves_a_different_session() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(&temp, "g_conditional_stop", "peer1", "sleep 30")).expect("start");
    assert!(
        stop_if_started_at("g_conditional_stop", "peer1", "stale")
            .expect("conditional stop")
            .is_none()
    );
    assert!(
        status("g_conditional_stop", "peer1")
            .expect("status")
            .running
    );
    stop("g_conditional_stop", "peer1").expect("cleanup");
}

#[test]
fn restarts_a_naturally_exited_session_without_reap() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(&temp, "g_restart_exited", "peer1", "exit 0")).expect("first");
    for _ in 0..100 {
        if !status("g_restart_exited", "peer1").expect("status").running {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!status("g_restart_exited", "peer1").expect("status").running);
    start(spec(&temp, "g_restart_exited", "peer1", "sleep 30")).expect("restart");
    stop("g_restart_exited", "peer1").expect("cleanup");
}

#[test]
fn write_rejects_a_naturally_exited_session_before_reap() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = "g_write_exited";
    let actor_id = "peer1";
    start(spec(&temp, group_id, actor_id, "exit 0")).expect("start");
    for _ in 0..100 {
        if !status(group_id, actor_id).expect("status").running {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(matches!(
        write(group_id, actor_id, b"must-not-be-reported-as-delivered"),
        Err(RuntimeError::NotFound(group, actor))
            if group == group_id && actor == actor_id
    ));
    stop(group_id, actor_id).expect("cleanup");
}

#[test]
fn reap_does_not_report_a_session_replaced_after_its_snapshot() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group_id = "g_reap_replaced";
    let actor_id = "peer1";
    start(spec(&temp, group_id, actor_id, "exit 0")).expect("first");
    for _ in 0..100 {
        if !status(group_id, actor_id).expect("status").running {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let previous = lookup(group_id, actor_id).expect("previous session");
    let (previous_status, previous_history) = {
        let mut session = previous.lock().expect("previous lock");
        session.finish_output().expect("finish previous output");
        (session.status(), session.history_handle())
    };

    start(spec(&temp, group_id, actor_id, "sleep 30")).expect("replacement");
    let exited = commit_reaped(vec![(
        (group_id.into(), actor_id.into()),
        previous,
        previous_history,
        previous_status,
    )])
    .expect("commit reap snapshot");

    assert!(exited.is_empty());
    assert!(
        status(group_id, actor_id)
            .expect("replacement status")
            .running
    );
    stop(group_id, actor_id).expect("cleanup");
}

#[test]
fn stop_is_bounded_when_a_background_child_keeps_the_pty_open() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(
        &temp,
        "g_background_child",
        "peer1",
        "trap '' HUP; sleep 3 & echo $! > background.pid",
    ))
    .expect("start");
    let pid_path = temp.path().join("background.pid");
    for _ in 0..100 {
        if pid_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let background_pid = std::fs::read_to_string(&pid_path)
        .expect("background pid")
        .trim()
        .parse::<i32>()
        .expect("numeric background pid");
    for _ in 0..100 {
        if !status("g_background_child", "peer1")
            .expect("status")
            .running
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !status("g_background_child", "peer1")
            .expect("status")
            .running
    );

    let started = std::time::Instant::now();
    let result = stop("g_background_child", "peer1");
    let elapsed = started.elapsed();

    result.expect("stop");
    assert!(elapsed < Duration::from_secs(1), "stop took {elapsed:?}");
    for _ in 0..100 {
        if nix::sys::signal::kill(nix::unistd::Pid::from_raw(background_pid), None).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("background process {background_pid} survived actor stop");
}

#[test]
fn stopped_reader_cannot_append_after_the_next_session_starts() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_dir = temp.path().join("terminal");
    let history = |name: &str| HistoryConfig {
        path: actor_dir.join(format!("{name}.pty")),
        max_bytes: 1024 * 1024,
        hot_bytes: 1024,
        persist: true,
    };
    start_with_history(
        spec(
            &temp,
            "g_reader_boundary",
            "peer1",
            "trap '' HUP; (sleep 1; printf late-old) & printf early-old",
        ),
        history("old"),
    )
    .expect("first");
    for _ in 0..100 {
        if !status("g_reader_boundary", "peer1")
            .expect("status")
            .running
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    stop("g_reader_boundary", "peer1").expect("stop first");

    start_with_history(
        spec(
            &temp,
            "g_reader_boundary",
            "peer1",
            "printf new-session; sleep 2",
        ),
        history("new"),
    )
    .expect("second");
    std::thread::sleep(Duration::from_millis(1_200));

    let old = std::fs::read(actor_dir.join("old.pty")).expect("old transcript");
    assert!(!String::from_utf8_lossy(&old).contains("late-old"));
    let page = crate::read_latest_page(&actor_dir, None, 1024).expect("history");
    assert_eq!(page.data, "early-oldnew-session");
    stop("g_reader_boundary", "peer1").expect("cleanup");
}

#[test]
fn memory_only_replacement_continues_the_actor_cursor() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let history_config = |name: &str| HistoryConfig {
        path: temp.path().join(format!("{name}.pty")),
        max_bytes: 1024 * 1024,
        hot_bytes: 1024,
        persist: false,
    };
    start_with_history(
        spec(
            &temp,
            "g_memory_cursor",
            "peer1",
            "printf old-session; sleep 30",
        ),
        history_config("old"),
    )
    .expect("first session");
    for _ in 0..100 {
        if history("g_memory_cursor", "peer1", None, 1024)
            .is_ok_and(|page| page.data.contains("old-session"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    stop("g_memory_cursor", "peer1").expect("stop first session");
    let old_end = history("g_memory_cursor", "peer1", None, 1024)
        .expect("completed history")
        .end_cursor;

    start_with_history(
        spec(
            &temp,
            "g_memory_cursor",
            "peer1",
            "printf replacement-session; sleep 30",
        ),
        history_config("replacement"),
    )
    .expect("replacement session");
    let mut replacement = None;
    for _ in 0..100 {
        let page =
            history_since("g_memory_cursor", "peer1", old_end, 1024).expect("replacement history");
        if page.data.contains("replacement-session") {
            replacement = Some(page);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let replacement = replacement.expect("replacement output");
    assert_eq!(replacement.start_cursor, old_end);
    assert!(!replacement.cursor_expired);
    stop("g_memory_cursor", "peer1").expect("cleanup");
}

#[test]
fn submit_delay_stops_promptly_when_cancelled() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(&temp, "g_cancel_submit", "peer1", "sleep 30")).expect("start");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let started = std::time::Instant::now();
    let worker = std::thread::spawn(move || {
        submit_interruptible(
            "g_cancel_submit",
            "peer1",
            b"echo delayed",
            b"\r",
            Duration::from_secs(5),
            &worker_cancelled,
        )
        .expect("submit")
    });
    std::thread::sleep(Duration::from_millis(50));
    cancelled.store(true, Ordering::Release);
    assert!(!worker.join().expect("join"));
    assert!(started.elapsed() < Duration::from_millis(500));
    stop("g_cancel_submit", "peer1").expect("cleanup");
}

#[test]
fn submit_sequence_writes_each_key_in_order() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    start(spec(
        &temp,
        "g_submit_sequence",
        "peer1",
        "stty raw -echo; printf '\\033[?2004h'; dd bs=1 count=3 2>/dev/null | od -An -t x1",
    ))
    .expect("start");
    assert!(
        super::wait_for_input_ready(
            "g_submit_sequence",
            "peer1",
            Duration::from_secs(3),
            &AtomicBool::new(false),
        )
        .expect("raw terminal ready")
    );
    assert!(
        submit_sequence_interruptible(
            "g_submit_sequence",
            "peer1",
            b"x",
            &[b"\r", b"\r"],
            Duration::ZERO,
            Duration::ZERO,
            &AtomicBool::new(false),
        )
        .expect("submit")
    );
    for _ in 0..100 {
        if !status("g_submit_sequence", "peer1")
            .expect("status")
            .running
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Stop joins the output reader before inspecting the completed transcript.
    stop("g_submit_sequence", "peer1").expect("cleanup");
    let output = history("g_submit_sequence", "peer1", None, 1024)
        .expect("history")
        .data;
    let tokens = output.split_ascii_whitespace().collect::<Vec<_>>();
    assert!(
        tokens.windows(3).any(|items| items == ["78", "0d", "0d"]),
        "{output:?}"
    );
}

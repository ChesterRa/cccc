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
        "stty raw -echo; dd bs=1 count=3 2>/dev/null | od -An -t x1",
    ))
    .expect("start");
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
    let output = history("g_submit_sequence", "peer1", None, 1024)
        .expect("history")
        .data;
    let tokens = output.split_ascii_whitespace().collect::<Vec<_>>();
    assert!(
        tokens.windows(3).any(|items| items == ["78", "0a", "0a"]),
        "{output:?}"
    );
    stop("g_submit_sequence", "peer1").expect("cleanup");
}

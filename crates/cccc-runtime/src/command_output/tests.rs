use super::*;

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}

#[tokio::test]
async fn drains_both_streams_while_feeding_input_and_preserves_exit_status() {
    let input = vec![b'x'; 262_144];
    let output = capture_command(
        &mut shell("head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; cat; exit 7"),
        Some(&input),
        Duration::from_secs(5),
        600_000,
    )
    .await
    .expect("concurrent pipes");
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout.len(), 524_288);
    assert_eq!(&output.stdout[262_144..], input.as_slice());
    assert_eq!(output.stderr, vec![0; 262_144]);
    assert!(!output.stdout_truncated && !output.stderr_truncated);
}

#[tokio::test]
async fn output_is_bounded_while_excess_is_drained() {
    let output = capture_command(
        &mut shell("head -c 3000000 /dev/zero; head -c 3000000 /dev/zero >&2; exit 9"),
        None,
        Duration::from_secs(5),
        1024,
    )
    .await
    .expect("bounded capture");
    assert_eq!(output.status.code(), Some(9));
    assert_eq!(output.stdout.len(), 1024);
    assert_eq!(output.stderr.len(), 1024);
    assert!(output.stdout_truncated && output.stderr_truncated);
    let exact = capture_command(
        &mut shell("printf abcd; printf efgh >&2"),
        None,
        Duration::from_secs(2),
        4,
    )
    .await
    .expect("exact limit");
    assert_eq!(exact.stdout, b"abcd");
    assert!(!exact.stdout_truncated && !exact.stderr_truncated);
}

#[tokio::test]
async fn unbounded_producer_still_times_out_after_capture_limit() {
    let started = std::time::Instant::now();
    let error = capture_command(&mut shell("exec yes"), None, Duration::from_millis(100), 64)
        .await
        .expect_err("timeout");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn successful_exit_revokes_descendants_and_preserves_parent_output() {
    let started = std::time::Instant::now();
    let output = capture_command(
        &mut shell("sleep 3 & printf done"),
        None,
        Duration::from_secs(1),
        1024,
    )
    .await
    .expect("parent output");
    assert_eq!(output.stdout, b"done");
    assert!(output.status.success());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn timeout_covers_input_even_when_child_does_not_read() {
    let error = capture_command(
        &mut shell("sleep 3"),
        Some(&vec![b'x'; 262_144]),
        Duration::from_millis(50),
        1024,
    )
    .await
    .expect_err("input timeout");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn stdin_is_closed_after_writing_or_when_absent() {
    let output = capture_command(
        &mut shell("cat; printf eof"),
        Some(b"input"),
        Duration::from_secs(2),
        1024,
    )
    .await
    .expect("stdin EOF");
    assert_eq!(output.stdout, b"inputeof");
    let output = capture_command(
        &mut shell("cat; printf eof"),
        None,
        Duration::from_secs(2),
        1024,
    )
    .await
    .expect("null stdin");
    assert_eq!(output.stdout, b"eof");
}

#[tokio::test]
async fn cancelling_a_polled_future_terminates_the_owned_tree() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut command = shell("sleep 0.3; printf late > marker");
    command.current_dir(temp.path());
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            capture_command(&mut command, None, Duration::from_secs(5), 1024)
        )
        .await
        .is_err()
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(!temp.path().join("marker").exists());
}

#[tokio::test]
async fn blocking_adapter_is_safe_inside_an_existing_runtime() {
    let output = capture_command_blocking(
        &mut shell("printf nested"),
        None,
        Duration::from_secs(2),
        1024,
    )
    .expect("nested adapter");
    assert_eq!(output.stdout, b"nested");
}

#[test]
fn blocking_adapter_preserves_cwd_environment_and_spawn_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut command = shell("printf '%s' \"$CAPTURE_FIXTURE\"; pwd");
    command
        .env("CAPTURE_FIXTURE", "value:")
        .current_dir(temp.path());
    let output = capture_command_blocking(&mut command, None, Duration::from_secs(2), 1024)
        .expect("configured command");
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8"),
        format!("value:{}\n", temp.path().display())
    );
    let error = capture_command_blocking(
        &mut Command::new(temp.path().join("absent")),
        None,
        Duration::from_secs(1),
        1024,
    )
    .expect_err("missing executable");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[cfg(target_os = "linux")]
#[test]
fn repeated_completion_timeout_and_cancellation_release_processes_and_pipes() {
    const CHILD: &str = "CCCC_TEST_COMMAND_CAPTURE_LIFECYCLE";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "command_output::tests::repeated_completion_timeout_and_cancellation_release_processes_and_pipes", "--nocapture"])
            .env(CHILD, "1").output().expect("isolated capture fixture");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        capture_command(
            &mut shell("printf warm"),
            None,
            Duration::from_secs(1),
            1024,
        )
        .await
        .expect("warm up");
        let descriptors = || std::fs::read_dir("/proc/self/fd").expect("fds").count();
        let baseline = descriptors();
        for _ in 0..30 {
            let completed = capture_command(
                &mut shell("printf done; printf err >&2"),
                None,
                Duration::from_secs(1),
                1024,
            )
            .await
            .expect("completed capture");
            assert_eq!(completed.stdout, b"done");
            let timed_out = capture_command(
                &mut shell("sleep 3 & wait"),
                None,
                Duration::from_millis(10),
                1024,
            )
            .await
            .expect_err("timeout");
            assert_eq!(timed_out.kind(), io::ErrorKind::TimedOut);
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(10),
                    capture_command(
                        &mut shell("sleep 3 & wait"),
                        None,
                        Duration::from_secs(1),
                        1024
                    )
                )
                .await
                .is_err()
            );
            assert!(
                std::fs::read_to_string("/proc/thread-self/children")
                    .expect("children")
                    .trim()
                    .is_empty(),
                "child process was not reaped"
            );
            assert_eq!(descriptors(), baseline, "pipe or reactor descriptor leaked");
        }
        println!(
            "30 rounds / 90 commands: descriptors {baseline} -> {}, no unreaped children",
            descriptors()
        );
    });
}

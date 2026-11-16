use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CONSOLE_TEST: &str =
    "process_tree::windows_tests::background_children_have_no_console_and_keep_pipe_io";
const CONSOLE_TEST_MODE: &str = "CCCC_WINDOWS_CONSOLE_TEST_MODE";
const PIPE_INPUT: &[u8] = b"pipe-round-trip\n";
// PowerShell is a console executable. P/Invoke here observes that child's real
// console, without adding unsafe code or a production API just for this test.
const CONSOLE_PROBE: &str = r#"
Add-Type -Namespace CcccProbe -Name Native -MemberDefinition '[DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();'
Write-Output ('CCCC_NO_CONSOLE=' + ([CcccProbe.Native]::GetConsoleWindow() -eq [IntPtr]::Zero))
Write-Output ('CCCC_INPUT=' + [Console]::ReadLine())
"#;

fn console_probe(mut command: Command) -> Command {
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        CONSOLE_PROBE,
    ]);
    command
}

fn assert_console_probe(output: std::process::Output, no_console: bool) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "probe failed: {stdout}\n{stderr}");
    let expected = format!(
        "CCCC_NO_CONSOLE={}",
        if no_console { "True" } else { "False" }
    );
    assert!(
        stdout.lines().any(|line| line == expected),
        "expected {expected}: {stdout}\n{stderr}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line == "CCCC_INPUT=pipe-round-trip"),
        "pipe input lost: {stdout}\n{stderr}"
    );
}

fn pipe_bytes(mut pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).expect("drain fixture output");
    bytes
}

fn wait_with_pipes(
    mut child: std::process::Child,
    tree: OwnedProcessTree,
    input: &[u8],
) -> std::process::Output {
    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).expect("write fixture input");
    }
    // Drain before waiting: a failure's diagnostics may exceed the pipe buffer.
    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");
    let stdout = std::thread::spawn(move || pipe_bytes(stdout));
    let stderr = std::thread::spawn(move || pipe_bytes(stderr));
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = tree.try_wait(|| child.try_wait()).expect("poll fixture") {
            break status;
        }
        if Instant::now() >= deadline {
            tree.terminate().expect("stop timed-out fixture");
            let _ = child.wait();
            panic!("console fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    std::process::Output {
        status,
        stdout: stdout.join().expect("stdout reader"),
        stderr: stderr.join().expect("stderr reader"),
    }
}

#[test]
fn background_children_have_no_console_and_keep_pipe_io() {
    if std::env::var(CONSOLE_TEST_MODE).as_deref() == Ok("detached") {
        // The raw control must detect a console before the hidden branches can prove anything.
        // Both spawn directly; the parent's Job covers descendants and the outer timeout
        // terminates that entire tree if one of these probes stalls.
        for (command, no_console) in [
            (Command::new("powershell.exe"), false),
            (background_command("powershell.exe"), true),
        ] {
            let mut child = console_probe(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn direct console probe");
            use std::io::Write;
            child
                .stdin
                .take()
                .expect("direct stdin")
                .write_all(PIPE_INPUT)
                .expect("write direct input");
            assert_console_probe(child.wait_with_output().expect("direct output"), no_console);
        }

        let mut command = console_probe(Command::new("powershell.exe"));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (child, tree) = OwnedProcessTree::spawn(&mut command).expect("spawn background probe");
        assert_console_probe(wait_with_pipes(child, tree, PIPE_INPUT), true);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let captured = crate::capture_command_with_start_evidence(
                &mut console_probe(Command::new("powershell.exe")),
                Some(PIPE_INPUT),
                Duration::from_secs(20),
                4096,
            )
            .await
            .expect("capture background probe");
            assert_console_probe(
                std::process::Output {
                    status: captured.status,
                    stdout: captured.stdout,
                    stderr: captured.stderr,
                },
                true,
            );

            // This is the same constructor used by stdio MCP servers.
            let mut command = background_tokio_command("powershell.exe");
            command
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    CONSOLE_PROBE,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let mut child = command.spawn().expect("spawn asynchronous probe");
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take().expect("asynchronous stdin");
            stdin
                .write_all(PIPE_INPUT)
                .await
                .expect("write asynchronous input");
            drop(stdin);
            let output = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
                .await
                .expect("asynchronous probe timeout")
                .expect("asynchronous output");
            assert_console_probe(output, true);
        });
        println!("CCCC_DETACHED_PROBES_PASSED");
        return;
    }

    // Reproduce a detached daemon, independently of the test runner's console.
    // Without NO_WINDOW its console children allocate their own console.
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", CONSOLE_TEST, "--nocapture", "--test-threads=1"])
        .env(CONSOLE_TEST_MODE, "detached")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, tree) = OwnedProcessTree::spawn_with_creation_flags(&mut command, 0x0000_0008)
        .expect("spawn detached parent fixture");
    let output = wait_with_pipes(child, tree, &[]);
    assert!(
        output.status.success(),
        "detached fixture failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("CCCC_DETACHED_PROBES_PASSED"));
}

#[test]
fn job_termination_kills_descendants_while_child_lock_is_held() {
    let (mut child, owner) = OwnedProcessTree::spawn(
        Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1"])
            .stdout(Stdio::piped()),
    )
    .expect("spawn child process");
    let mut stdout = BufReader::new(child.stdout.take().expect("take owned test resource"));
    let ready_bytes = stdout
        .read_line(&mut String::new())
        .expect("read descendant readiness");
    assert!(
        ready_bytes > 0,
        "ping must start before testing Job closure"
    );
    let child = Mutex::new(child);
    let mut held_child = child.lock().expect("lock test state");
    assert!(
        owner
            .try_wait(|| held_child.try_wait())
            .expect("poll live parent")
            .is_none(),
        "cmd must still be running before Job closure"
    );
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut output));
    });
    assert!(
        matches!(
            rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "the live process tree must keep stdout open before Job closure"
    );
    owner.terminate().expect("terminate owned process tree");
    rx.recv_timeout(Duration::from_secs(5))
        .expect("cmd or ping survived Job closure")
        .expect("drain stdout after Job closure");
    // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE promises termination, not a nonzero
    // exit code. In Windows CI the terminated cmd returned zero. Verify the
    // running -> exited transition directly, with no exit-code assumption.
    let deadline = Instant::now() + Duration::from_secs(5);
    while held_child
        .try_wait()
        .expect("poll terminated parent")
        .is_none()
    {
        assert!(Instant::now() < deadline, "cmd survived Job closure");
        std::thread::sleep(Duration::from_millis(10));
    }
    held_child.wait().expect("reap terminated parent");
    owner.terminate().expect("terminate owned process tree");
}

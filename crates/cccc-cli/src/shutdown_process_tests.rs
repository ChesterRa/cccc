use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

#[test]
fn forced_launcher_exit_terminates_owned_tree_without_destructors() {
    const CHILD: &str = "CCCC_TEST_FORCED_LAUNCHER";
    if std::env::var_os(CHILD).is_some() {
        let (_child, tree) = cccc_runtime::OwnedProcessTree::spawn(
            Command::new("sh")
                .args([
                    "-c",
                    "sleep 60 & printf 'CCCC_FORCE_READY:%s:%s\\n' \"$$\" \"$!\"; wait",
                ])
                .stdout(Stdio::inherit()),
        )
        .expect("spawn child process");
        // The terminator remains inaccessible behind an ordinary owner lock;
        // process::exit must work without taking that lock or running Drop.
        let owner = std::sync::Mutex::new(tree);
        let _held_owner = owner.lock().expect("lock test state");
        let mut go = String::new();
        std::io::stdin()
            .read_line(&mut go)
            .expect("read child readiness");
        force_exit();
    }
    if !fixture_pidfds_available() {
        eprintln!("skipping: cannot safely pin fixture processes");
        return;
    }
    let mut launcher = FixtureLauncher(Command::new(std::env::current_exe().expect("locate test executable"))
        .args(["--exact", "shutdown::process_tests::forced_launcher_exit_terminates_owned_tree_without_destructors", "--nocapture"])
        .env(CHILD, "1").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("spawn child process"));
    let stdout = launcher.0.stdout.take().expect("take owned test resource");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.expect("fixture value must be available");
            if let Some(pids) = line.strip_prefix("CCCC_FORCE_READY:") {
                let (leader, member) = pids.split_once(':').expect("fixture PIDs");
                let _ = ready_tx.send((
                    leader.parse::<i32>().expect("fixture leader PID"),
                    member.parse::<i32>().expect("fixture member PID"),
                ));
            }
        }
        let _ = eof_tx.send(());
    });
    let (leader, member) = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actor ready");
    let _leader = FixtureProcess::pin(leader, launcher.0.id()).expect("pin fixture leader");
    let _member = FixtureProcess::pin(member, leader as u32).expect("pin fixture member");
    launcher
        .0
        .stdin
        .take()
        .expect("take owned test resource")
        .write_all(b"go\n")
        .expect("write process input");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = launcher.0.try_wait().expect("poll child process") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = launcher.0.kill();
            launcher.0.wait().expect("reap child process");
            panic!("forced launcher exit blocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let closed = eof_rx.recv_timeout(Duration::from_secs(5));
    assert_eq!(status.code(), Some(INTERRUPTED_EXIT_CODE));
    closed.expect("actor or descendant survived process::exit and still owns stdout");
}

#[test]
fn the_host_watcher_catches_an_interrupt_before_it_first_runs() {
    in_fresh_process(
        "shutdown::process_tests::the_host_watcher_catches_an_interrupt_before_it_first_runs",
        "CCCC_TEST_HOST_EARLY_INTERRUPT",
        || interrupt_before_first_run(|controller| watch_for_interrupt(controller, None)),
    );
}

#[test]
fn the_terminal_watcher_catches_an_interrupt_before_it_first_runs() {
    in_fresh_process(
        "shutdown::process_tests::the_terminal_watcher_catches_an_interrupt_before_it_first_runs",
        "CCCC_TEST_TERMINAL_EARLY_INTERRUPT",
        || interrupt_before_first_run(watch_signals),
    );
}

/// Runs `scenario` in a fresh run of this test alone, where nothing else has
/// registered for interrupts and an interrupt nobody catches ends that run.
fn in_fresh_process(test: &str, marker: &str, scenario: impl FnOnce()) {
    if std::env::var_os(marker).is_some() {
        scenario();
        return;
    }
    let status = Command::new(std::env::current_exe().expect("locate test executable"))
        .args(["--exact", test, "--nocapture"])
        .env(marker, "1")
        .status()
        .expect("run the test in a fresh process");
    assert!(status.success(), "the fresh run failed: {status:?}");
}

/// A current-thread runtime runs a spawned watcher only once this code yields,
/// so the interrupt is raised before the watcher has run at all.
fn interrupt_before_first_run(
    watch: impl FnOnce(Controller) -> std::io::Result<tokio::task::JoinHandle<()>>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let controller = Controller::new();
        let watcher = watch(controller.clone()).expect("listen for interrupts");
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT).expect("raise an interrupt");
        let requested = tokio::time::timeout(Duration::from_secs(5), controller.requested()).await;
        watcher.abort();
        requested.expect("the interrupt asked for a stop");
    });
}

/// Reap the direct fixture child on every exit, including assertion failure.
struct FixtureLauncher(std::process::Child);

impl Drop for FixtureLauncher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[path = "../../test_support/fixture_process.rs"]
mod fixture_process;
use fixture_process::{FixtureProcess, fixture_pidfds_available};

#[test]
fn second_terminal_interrupt_restores_modes_before_process_exit() {
    use crate::host_state::{DaemonState, HostPhase, HostState, WebState};
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::sync::{Arc, Mutex};
    const CHILD: &str = "CCCC_TEST_TUI_FORCE_EXIT";
    let test =
        "shutdown::process_tests::second_terminal_interrupt_restores_modes_before_process_exit";
    if std::env::var_os(CHILD).is_some() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let home = HomeLayout::resolve().expect("isolated home");
            home.initialize().expect("initialize");
            let mut control = TerminalControl::new().expect("signals");
            let (state, receiver) = tokio::sync::watch::channel(HostState {
                daemon: DaemonState::StartedHere {
                    pid: std::process::id(),
                },
                web: WebState::Starting,
                phase: HostPhase::Running,
            });
            let (intents, receiver_intents) = tokio::sync::mpsc::unbounded_channel();
            control.connect(receiver_intents);
            let _interface = crate::tui::start(home.clone(), receiver, intents)
                .await
                .expect("terminal interface");
            std::fs::write(home.root().join("force-ready"), "ready").expect("ready");
            // Deliberately do not perform ordinary shutdown: the second real
            // key event must reach the same controller and use its force path.
            let _keep_state = state;
            std::future::pending::<()>().await;
        });
        unreachable!("force exit returns no value");
    }
    let temp = tempfile::tempdir().expect("isolated root");
    let home = temp.path().join("cccc");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("terminal");
    let modes = format!("{:?}", pair.master.get_termios());
    let mut command = CommandBuilder::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", test, "--nocapture"]);
    command.env_clear();
    command.env(CHILD, "1");
    command.env("HOME", temp.path());
    command.env("CCCC_HOME", &home);
    command.env("CODEX_HOME", temp.path().join("codex"));
    command.env("CLAUDE_CONFIG_DIR", temp.path().join("claude"));
    command.env("PI_CODING_AGENT_DIR", temp.path().join("pi"));
    command.env("XDG_CONFIG_HOME", temp.path().join("config"));
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).expect("force fixture");
    drop(pair.slave);
    let mut writer = pair.master.take_writer().expect("writer");
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let output = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(count) = std::io::Read::read(&mut reader, &mut buffer) {
            if count == 0 {
                break;
            }
            captured
                .lock()
                .expect("output")
                .extend_from_slice(&buffer[..count]);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !home.join("force-ready").exists() {
        if Instant::now() >= deadline || child.try_wait().expect("status").is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "terminal fixture not ready: {}",
                String::from_utf8_lossy(&output.lock().expect("output"))
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    writer
        .write_all(b"\x03\x03")
        .expect("two real interrupt keys");
    writer.flush().expect("flush");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("status") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("second interrupt did not force exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.exit_code(), INTERRUPTED_EXIT_CODE as u32);
    assert_eq!(format!("{:?}", pair.master.get_termios()), modes);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let captured = output.lock().expect("output");
        if captured.windows(8).any(|bytes| bytes == b"\x1b[?1049l") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "force exit did not leave alternate screen"
        );
        drop(captured);
        std::thread::sleep(Duration::from_millis(10));
    }
}

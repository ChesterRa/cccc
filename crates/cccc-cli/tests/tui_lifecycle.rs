//! Exercise the real CLI through an OS terminal, including Windows ConPTY.
//! All state and provider homes are disposable; no provider is invoked.
#[path = "tui_lifecycle/published.rs"]
mod published;

#[path = "tui_lifecycle/terminal.rs"]
mod terminal;

use fs2::FileExt;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use terminal::Terminal;

struct Fixture {
    files: published::PublishedFiles,
    root: tempfile::TempDir,
    home: PathBuf,
    port: u16,
}

impl Fixture {
    fn new() -> Self {
        Self::with_reserved_port().0
    }

    /// The returned listener keeps holding the Web port until it is dropped.
    /// Every launch also gets this port in `CCCC_WEB_PORT`, so none can fall
    /// back to the default port. A port released here can be taken by
    /// another socket before the child binds it, so launches that do not test
    /// the configured port pass `--port 0` and read the bound port back.
    fn with_reserved_port() -> (Self, std::net::TcpListener) {
        let root = tempfile::tempdir().expect("isolated root");
        let home = root.path().join("cccc");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = listener.local_addr().expect("address").port();
        std::fs::create_dir_all(&home).expect("home");
        let files = published::PublishedFiles::new(root.path());
        (
            Self {
                files,
                root,
                home,
                port,
            },
            listener,
        )
    }

    fn env(&self) -> Vec<(String, String)> {
        let mut env: Vec<_> = [
            "PATH",
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "PATHEXT",
            "TMP",
            "TEMP",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
        .collect();
        for (key, path) in [
            ("HOME", self.root.path().join("user")),
            ("USERPROFILE", self.root.path().join("user")),
            ("CCCC_HOME", self.home.clone()),
            ("CODEX_HOME", self.root.path().join("codex")),
            ("CLAUDE_CONFIG_DIR", self.root.path().join("claude")),
            ("PI_CODING_AGENT_DIR", self.root.path().join("pi")),
            ("XDG_CONFIG_HOME", self.root.path().join("config")),
            ("XDG_DATA_HOME", self.root.path().join("data")),
            ("XDG_STATE_HOME", self.root.path().join("state")),
        ] {
            std::fs::create_dir_all(&path).expect("isolated directory");
            env.push((key.into(), path.to_string_lossy().into_owned()));
        }
        // Windows resolves AppData through known folders and verifies that
        // they exist, even when CCCC_HOME is an explicit absolute path.
        #[cfg(windows)]
        for leaf in ["Roaming", "Local"] {
            std::fs::create_dir_all(self.root.path().join("user/AppData").join(leaf))
                .expect("isolated Windows known folder");
        }
        env.extend([
            ("CCCC_WEB_HOST".into(), "127.0.0.1".into()),
            ("CCCC_WEB_PORT".into(), self.port.to_string()),
            ("TERM".into(), "xterm-256color".into()),
            ("NO_COLOR".into(), "1".into()),
        ]);
        env
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cccc"));
        command
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(self.root.path());
        command
    }

    fn group(
        &self,
        title: &str,
        state: cccc_contracts::GroupState,
    ) -> (cccc_core::GroupStore, cccc_core::GroupDoc) {
        let home = cccc_core::HomeLayout::from_path(&self.home).expect("fixture Home");
        let store = cccc_core::GroupStore::new(home).expect("fixture store");
        let mut group = store.create(title, "").expect("fixture Group");
        group.running = state != cccc_contracts::GroupState::Stopped;
        group.state = state;
        // Empty Groups exercise lifecycle intent independently of observed
        // Actor presence and cannot launch a provider during these tests.
        store.save(&group).expect("fixture lifecycle");
        (store, group)
    }

    fn start(&self, args: &[&str], extra: &[(&str, &str)]) -> Terminal {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("OS terminal");
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_cccc"));
        command.args(args);
        command.env_clear();
        for (key, value) in self.env() {
            command.env(key, value);
        }
        for (key, value) in extra {
            command.env(key, value);
        }
        command.cwd(self.root.path());
        let on_exit = self.files.on_exit();
        Terminal::start(pair, command, self.home.clone(), on_exit)
    }

    /// The Web lock is held until the process exits, so a free lock proves its
    /// listener is gone. A probe of the port would not: it is anyone's by then.
    fn assert_stopped(&self) {
        for filename in ["ccccd.lock", "cccc-web.lock"] {
            assert_lock_free(&self.home.join("daemon").join(filename));
        }
        assert!(
            !self.home.join("daemon/ccccd.addr.json").exists(),
            "daemon descriptor remained"
        );
    }

    /// Waits for Web to record the port it bound in its runtime descriptor.
    fn web_port(&self) -> u16 {
        self.published_web_port()
            .expect("Web host published a valid port")
    }

    fn published_web_port(&self) -> std::io::Result<u16> {
        let path = self.home.join("daemon/web_runtime.json");
        let bytes = self.files.read(&path)?;
        let runtime: serde_json::Value = serde_json::from_slice(&bytes)?;
        runtime["port"]
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port > 0)
            .ok_or_else(|| std::io::Error::other("Web runtime descriptor has no bound port"))
    }

    fn external_daemon(
        &self,
    ) -> (
        std::process::Child,
        std::thread::JoinHandle<std::io::Result<std::io::Sink>>,
    ) {
        let mut daemon = self
            .command(&["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("isolated external daemon");
        let reader = self.files.observe_pipe(
            daemon.stdout.take().expect("external daemon stdout"),
            std::io::sink(),
        );
        (daemon, reader)
    }

    fn wait_for_daemon_address(&self) {
        let bytes = self
            .files
            .read(&self.home.join("daemon/ccccd.addr.json"))
            .expect("external daemon published its address");
        serde_json::from_slice::<cccc_contracts::DaemonAddress>(&bytes)
            .expect("external daemon address");
    }
}

/// The port in the Web banner's `Local:` line. The banner is printed after
/// the listener binds and before the runtime descriptor is written.
#[cfg(unix)]
fn banner_port(line: &str) -> Option<u16> {
    let (_, rest) = line.split_once("http://127.0.0.1:")?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
        .filter(|port| *port > 0)
}

#[test]
fn task_pages_and_completed_outcomes_are_readable_without_a_provider() {
    use cccc_core::{HomeLayout, context::ContextStore};
    use serde_json::{Map, Value, json};

    let fixture = Fixture::new();
    let (_store, group) = fixture.group("Task reader", cccc_contracts::GroupState::Active);
    let home = HomeLayout::from_path(&fixture.home).expect("isolated Home");
    let contexts = ContextStore::new(home).expect("contexts");
    let notes = format!("{}\nLAST_NOTES_LINE", "Full task notes 日本語\n".repeat(90));
    let mut operations: Vec<Map<String, Value>> = (1..=25)
        .map(|index| {
            json!({"op":"task.create","title":format!("Active task {index:02}"),
            "status":"active","notes":notes,
            "checklist":[{"text":"first","status":"done"},{"text":"second","status":"in_progress"}]
        }).as_object().expect("task op").clone()
        })
        .collect();
    for operation in [
        json!({"op":"task.create","title":"Planned task","status":"planned"}),
        json!({"op":"task.create","title":"Completed task","status":"done",
            "notes":"Completed task description","outcome":"FINAL_COMPLETED_OUTCOME"}),
    ] {
        operations.push(operation.as_object().expect("task op").clone());
    }
    contexts
        .sync(&group.group_id, &operations, None, "user", false)
        .expect("seed tasks");
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    terminal.until_screen("Task reader");
    terminal.send(b"\r");
    terminal.until_screen("Tasks: 25 active  1 planned  1 done");
    terminal.send(b"\r");
    terminal.until_screen("Active task 25");
    terminal.send(b"\r");
    terminal.until_screen("Checklist 1/2 done");
    terminal.send(b"\x1b[F");
    terminal.until_screen("LAST_NOTES_LINE");
    terminal.send(b"\x1b");
    terminal.until_screen("Left/Right tabs");
    terminal.send(b"\x1b[C");
    terminal.until_screen("Completed task");
    terminal.send(b"\r");
    terminal.until_screen("FINAL_COMPLETED_OUTCOME");
    terminal.send(b"\x1b");
    terminal.until_screen("Left/Right tabs");
    terminal.send(b"\x1b[D");
    terminal.until_screen("Active task 25");
    terminal.send(b"\x1b[6~");
    terminal.until_screen("Page 2");
    terminal.until_screen("Active task 05");
    terminal.send(b"\x1b");
    terminal.until_screen("Enter open");
    terminal.send(b"\x1b");
    terminal.until_screen("Native sessions");
    terminal.send(b"/quit\r");
    terminal.finish();
    fixture.assert_stopped();
}

#[cfg(target_os = "linux")]
#[test]
fn blocked_terminal_output_still_restores_input_when_closing_times_out() {
    use nix::pty::{Winsize, openpty};
    use nix::sys::signal::{Signal, kill};
    use nix::sys::termios::{FlowArg, LocalFlags, tcflow, tcgetattr};
    use nix::unistd::{Pid, ttyname};
    use rustix::event::{PollFd, PollFlags, poll};
    use std::os::unix::fs::OpenOptionsExt;

    let fixture = Fixture::new();
    let pair = openpty(
        Some(&Winsize {
            ws_row: 24,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )
    .expect("isolated PTY");
    let original = tcgetattr(&pair.slave).expect("original input mode");
    // A fresh open has its own file status flags. The child's stdout must stay blocking.
    let mut probe = OpenOptions::new()
        .write(true)
        .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY)
        .open(ttyname(&pair.slave).expect("slave device path"))
        .expect("independent nonblocking writer");
    let (stderr_closed, mut notify_closed) = std::io::pipe().expect("stderr completion signal");
    let mut child = fixture
        .command(&["--port", "0"])
        .stdin(Stdio::from(pair.slave.try_clone().expect("stdin")))
        .stdout(Stdio::from(pair.slave.try_clone().expect("stdout")))
        .stderr(Stdio::piped())
        .spawn()
        .expect("isolated host");
    let mut stderr = child.stderr.take().expect("host stderr");
    let finished = fixture.files.on_exit();
    let diagnostics = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = std::io::copy(&mut stderr, &mut bytes);
        finished(result.as_ref().err());
        // Wake the PTY wait even when the host exits without writing its first frame.
        let _ = notify_closed.write_all(&[1]);
        result.map(|_| bytes)
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut buffer = [0; 8192];
        let mut output = Vec::new();
        let marker = b"Native sessions";
        while !output.windows(marker.len()).any(|data| data == marker) {
            let mut fds = [
                PollFd::new(&pair.master, PollFlags::IN),
                PollFd::new(&stderr_closed, PollFlags::IN),
            ];
            rustix::io::retry_on_intr(|| poll(&mut fds, None)).expect("wait for first PTY output");
            assert!(
                fds[1].revents().is_empty(),
                "host closed stderr before its first screen"
            );
            assert!(
                fds[0].revents().contains(PollFlags::IN),
                "PTY hung up before its first screen"
            );
            let count =
                rustix::io::retry_on_intr(|| rustix::io::read(&pair.master, &mut buffer[..]))
                    .expect("read first screen");
            assert_ne!(count, 0, "PTY closed before its first screen");
            output.extend_from_slice(&buffer[..count]);
        }
        assert!(
            !tcgetattr(&pair.slave)
                .expect("raw mode")
                .local_flags
                .intersects(LocalFlags::ICANON | LocalFlags::ECHO)
        );
        fixture.web_port();
        tcflow(&pair.slave, FlowArg::TCOOFF).expect("pause terminal output");
        // A stopped driver may still queue bytes. The master is no longer drained;
        // proceed only once even a single byte would block on this slave.
        let blocked = loop {
            if let Err(error) = probe.write_all(b".") {
                break error;
            }
        };
        assert_eq!(blocked.kind(), std::io::ErrorKind::WouldBlock);

        kill(
            Pid::from_raw(child.id().try_into().expect("PID")),
            Signal::SIGTERM,
        )
        .expect("terminate only fixture host");
        let status = child.wait().expect("reap host");
        assert!(!status.success(), "terminal timeout must remain an error");
        let restored = tcgetattr(&pair.slave).expect("restored mode");
        for flag in [LocalFlags::ICANON, LocalFlags::ECHO] {
            assert_eq!(
                restored.local_flags.contains(flag),
                original.local_flags.contains(flag),
                "input flag {flag:?} remained changed while output was blocked"
            );
        }
        fixture.assert_stopped();
    }));
    let _ = tcflow(&pair.slave, FlowArg::TCOON);
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let diagnostics = diagnostics.join();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    let bytes = diagnostics
        .expect("host stderr reader")
        .expect("host stderr");
    let diagnostic = String::from_utf8_lossy(&bytes);
    assert!(
        diagnostic.contains("the terminal interface did not close within"),
        "missing terminal close failure: {diagnostic}"
    );
}

fn assert_lock_free(path: &Path) {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .expect("lock file");
    lock.try_lock_exclusive().expect("CCCC kept service lock");
    FileExt::unlock(&lock).expect("unlock");
}

#[test]
fn quit_stops_owned_services_and_restores_terminal() {
    // The one TUI launch without arguments, using the configured port. Its
    // reserved port is released before the child binds it, so another socket
    // can still take it in between.
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.until(&format!("http://127.0.0.1:{}", fixture.port));
    terminal.send(b"/quit\r");
    terminal.finish();
    assert!(terminal.text().contains("CCCC stopped."));
    assert!(
        terminal.text().contains("\x1b[?1049l"),
        "alternate screen remained active"
    );
    fixture.assert_stopped();
}

#[test]
fn group_controls_start_and_stop_an_empty_group_without_a_provider() {
    use cccc_contracts::GroupState;
    let fixture = Fixture::new();
    let title = "Control fixture";
    let (store, group) = fixture.group(title, GroupState::Stopped);
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    terminal.send(b"/groups\r");
    terminal.until_screen("Start group");
    terminal.until_screen(&group.group_id);
    terminal.send(b"\x1b[B\r");
    terminal.until_screen(&format!("Start {title}?"));
    terminal.send(b"\x1b[B\r");
    terminal.until_screen(&format!("{title}: start done."));
    let started = store.load(&group.group_id).expect("started Group");
    assert!(started.running, "an empty Group still records Start intent");
    assert_eq!(started.state, GroupState::Active);

    // Observed running is still false: Stop must remain an explicit action.
    terminal.until_screen("Stop group");
    terminal.send(b"\x1b[B\r");
    terminal.until_screen(&format!("Stop {title}?"));
    terminal.send(b"\x1b[B\r");
    terminal.until_screen(&format!("{title}: stop done."));
    let stopped = store.load(&group.group_id).expect("stopped Group");
    assert!(!stopped.running);
    assert_eq!(stopped.state, GroupState::Stopped);
    terminal.send(b"b/quit\r");
    terminal.finish();
    fixture.assert_stopped();

    let events = cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("Group events");
    assert_eq!(events.len(), 2, "polling must not append lifecycle events");
    assert_eq!(events[0].kind, "group.start");
    assert_eq!(events[1].kind, "group.stop");
    assert!(events.iter().all(|event| event.by == "user"));
}

#[test]
fn a_paused_group_can_be_stopped_directly_and_confirmation_is_cancellable() {
    use cccc_contracts::GroupState;
    let fixture = Fixture::new();
    let title = "Paused fixture";
    let (store, group) = fixture.group(title, GroupState::Paused);
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    terminal.send(b"/groups\r");
    terminal.until_screen("Stop group");
    terminal.until_screen(&group.group_id);
    terminal.send(b"\x1b[B\x1b[B\r");
    terminal.until_screen(&format!("Stop {title}?"));
    terminal.send(b"b");
    terminal.until_screen("Stop group");
    assert_eq!(
        store.load(&group.group_id).expect("still paused").state,
        GroupState::Paused
    );
    assert!(
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("no cancelled mutation")
            .is_empty()
    );
    terminal.send(b"\r");
    terminal.until_screen(&format!("Stop {title}?"));
    terminal.send(b"\x1b[B\r");
    terminal.until_screen(&format!("{title}: stop done."));
    assert_eq!(
        store.load(&group.group_id).expect("stopped Group").state,
        GroupState::Stopped
    );
    terminal.send(b"b/quit\r");
    terminal.finish();
    fixture.assert_stopped();

    let events = cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("Group events");
    assert_eq!(events.len(), 1, "Stop must not resume a paused Group first");
    assert_eq!(events[0].kind, "group.stop");
    assert_eq!(events[0].by, "user");
}

/// ConPTY can merge away a confirmation that queued input closes at once
/// (see `confirm_then`), so on Windows queued keys are also checked by their
/// outcome alone, once CCCC has exited and no stop can still be under way.
#[cfg(windows)]
fn queued_stop_keys_in_conpty(
    title: &str,
    keys: &[u8],
    done: Option<&str>,
) -> Vec<cccc_contracts::Event> {
    use cccc_contracts::GroupState;
    let fixture = Fixture::new();
    let (store, group) = fixture.group(title, GroupState::Active);
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until_screen(title);
    fixture.web_port();
    terminal.send(b"\r");
    terminal.until_screen(&group.group_id);
    terminal.send(b"\x1b[B\x1b[B");
    terminal.until_screen("> Stop group");
    terminal.send(keys);
    if let Some(done) = done {
        terminal.until_screen(done);
    }
    // Back on the main screen, every queued key has been read.
    terminal.send(b"b");
    terminal.until_screen("Tab menu");
    terminal.send(b"/quit\r");
    terminal.finish();
    fixture.assert_stopped();
    cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("Group events")
}

#[cfg(windows)]
#[test]
fn queued_enters_land_on_cancel_in_conpty() {
    let events = queued_stop_keys_in_conpty("Burst cancel fixture", b"\r\r", None);
    assert!(events.is_empty(), "a queued Enter confirmed: {events:?}");
}

#[cfg(windows)]
#[test]
fn a_deliberate_burst_still_stops_in_conpty() {
    let events = queued_stop_keys_in_conpty(
        "Burst stop fixture",
        b"\r\x1b[B\r",
        Some("Burst stop fixture: stop done."),
    );
    assert_eq!(events.len(), 1, "one burst stops the Group once");
    assert_eq!(events[0].kind, "group.stop");
    assert_eq!(events[0].by, "user");
}

#[test]
fn failed_web_start_reports_failure_after_stopping_its_daemon() {
    // Keep the reserved port occupied: releasing it and binding again let another
    // socket take it in between.
    let (fixture, _occupied) = Fixture::with_reserved_port();
    let mut terminal = fixture.start(&[], &[]);
    let status = terminal.finish_output();
    assert!(!status.success());
    let output = terminal.text();
    assert!(output.contains("CCCC stopped with problems."), "{output}");
    assert!(output.contains("failed:"));
    fixture.assert_stopped();
}

#[test]
fn raw_ctrl_c_stops_without_an_extra_enter() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    terminal.send(b"\x03");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn redraw_needs_no_cursor_reply_and_keeps_the_host_running() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    let before = terminal.mark();
    terminal.send(b"\x0c");
    // A new repaint arrives even though this terminal never answers cursor-position queries.
    terminal.until_after("Native sessions", before);
    terminal.send(b"/help\r");
    terminal.until_screen("On regular screens");
    terminal.send(b"\x1b");
    terminal.until_screen("Native sessions");
    terminal.send(b"/quit\r");
    terminal.finish();
    // Check the complete tail, including any query produced after the heading.
    assert!(
        !terminal.text_after(before).contains("\x1b[6n"),
        "repainting must not request the cursor position"
    );
    fixture.assert_stopped();
}

#[cfg(unix)]
#[test]
fn hangup_uses_the_same_graceful_cleanup() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(terminal.pid as i32),
        nix::sys::signal::Signal::SIGHUP,
    )
    .expect("hangup");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn explicit_launches_and_readonly_mode_keep_plain_output() {
    for (args, extra) in [
        (vec!["web", "--port", "0"], vec![]),
        (vec!["--headless", "--port", "0"], vec![]),
        (vec!["--port", "0"], vec![("CCCC_WEB_MODE", " ro ")]),
    ] {
        let fixture = Fixture::new();
        let mut terminal = fixture.start(&args, &extra);
        terminal.until("Local:");
        fixture.web_port();
        assert!(
            !terminal.text().contains("\x1b[?1049h"),
            "explicit/plain mode entered TUI"
        );
        let stop = fixture
            .command(&["daemon", "stop"])
            .output()
            .expect("stop owned daemon");
        assert!(
            stop.status.success(),
            "{}",
            String::from_utf8_lossy(&stop.stderr)
        );
        terminal.finish();
        fixture.assert_stopped();
    }
}

#[test]
fn a_port_option_keeps_the_tui_and_reports_the_actual_port() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    let port = fixture.web_port();
    terminal.until_screen(&format!("http://127.0.0.1:{port}"));
    assert!(terminal.text().contains("\x1b[?1049h"));
    terminal.send(b"/quit\r");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn a_small_window_cannot_accept_an_invisible_stop_confirmation() {
    use cccc_contracts::GroupState;
    let fixture = Fixture::new();
    let (store, group) = fixture.group("Small confirmation fixture", GroupState::Active);
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until("Native sessions");
    fixture.web_port();
    terminal.send(b"/groups\r");
    terminal.until_screen(&group.group_id);
    terminal.send(b"\x1b[B\x1b[B\r");
    terminal.until_screen("Stop Small confirmation fixture?");
    terminal.send(b"\x1b[B");
    terminal.until_screen("> Stop group");
    terminal.resize(PtySize {
        rows: 10,
        cols: 45,
        pixel_width: 0,
        pixel_height: 0,
    });
    terminal.send(b"\x0c");
    terminal.until_screen("Terminal too small");
    // Ctrl-C is the one accepted small-screen key. The PTY delivers it after the hidden
    // Enter and command; shutdown is the acknowledgement that the input queue progressed.
    terminal.send(b"\r/exit\r\x03");
    terminal.finish();
    let receipt = terminal.text();
    assert!(receipt.contains("CCCC stopped."), "{receipt}");
    assert!(
        !receipt.contains("had not finished when CCCC stopped"),
        "invisible action was submitted: {receipt}"
    );
    assert!(
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("Group events")
            .is_empty(),
        "invisible input committed a Group operation"
    );
    assert_eq!(
        store.load(&group.group_id).expect("untouched Group").state,
        GroupState::Active
    );
    fixture.assert_stopped();
}

#[test]
fn queued_group_confirmation_is_drawn_and_defaults_to_cancel() {
    use cccc_contracts::GroupState;
    let fixture = Fixture::new();
    let title = "Burst safety fixture";
    let (store, group) = fixture.group(title, GroupState::Active);
    let mut terminal = fixture.start(&["--port", "0"], &[]);
    terminal.until_screen(title);
    fixture.web_port();
    terminal.send(b"\r");
    terminal.until_screen(&group.group_id);
    terminal.send(b"\x1b[B\x1b[B");
    terminal.until_screen("> Stop group");

    terminal.confirm_then(b"\r", &format!("Stop {title}?"), b"\r");
    terminal.until_screen("Enter open");
    assert_eq!(
        store.load(&group.group_id).expect("unchanged Group").state,
        GroupState::Active
    );
    assert!(
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("no cancelled mutation")
            .is_empty()
    );

    // Even deliberate queued input must paint the confirmation first.
    terminal.confirm_then(b"\r", &format!("Stop {title}?"), b"\x1b[B\r");
    terminal.until_screen(&format!("{title}: stop done."));
    assert_eq!(
        store.load(&group.group_id).expect("stopped Group").state,
        GroupState::Stopped
    );
    terminal.send(b"b/quit\r");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn headless_does_not_accept_a_subcommand_or_create_state() {
    let fixture = Fixture::new();
    let output = fixture
        .command(&["--headless", "status"])
        .output()
        .expect("CLI validation");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("without a subcommand"));
    assert!(!fixture.home.join("daemon/ccccd.addr.json").exists());
}

#[test]
fn headless_ctrl_c_stops_owned_services_without_entering_the_tui() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&["--headless", "--port", "0"], &[]);
    terminal.until("Local:");
    fixture.web_port();
    assert!(!terminal.text().contains("\x1b[?1049h"));
    terminal.send(b"/setup\r");
    terminal.send(&[3]);
    terminal.finish();
    assert!(
        !terminal.text().contains("Pick apps"),
        "headless must not read commands"
    );
    fixture.assert_stopped();
}

#[cfg(unix)]
#[test]
fn headless_interrupt_while_the_web_starts_stops_without_the_forced_exit() {
    use std::io::BufRead;
    // As on Windows, the daemon is another process that never sees this
    // interrupt, so only the host can stop the Web server. The Web library's own
    // listener registers after its banner, so an interrupt sent on the banner
    // line usually arrives before it.
    let fixture = Fixture::new();
    let (mut daemon, daemon_output) = fixture.external_daemon();
    let mut host = None;
    let mut stderr_reader = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fixture.wait_for_daemon_address();
        let child = host.insert(
            fixture
                .command(&["--headless", "--port", "0"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("headless host"),
        );
        // The reader signals the banner as soon as it is read; EOF wakes the
        // receiver when a failed host never prints the banner.
        // The banner names the bound port; the runtime descriptor may not
        // exist yet when the interrupt arrives.
        let stderr = child.stderr.take().expect("host stderr");
        let (banner_tx, banner_rx) = std::sync::mpsc::channel();
        stderr_reader = Some(std::thread::spawn(move || {
            let mut banner_tx = Some(banner_tx);
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if line.contains("Local:")
                    && let Some(banner_tx) = banner_tx.take()
                {
                    let _ = banner_tx.send(banner_port(&line));
                }
            }
        }));
        banner_rx
            .recv()
            .expect("headless host exited before printing its banner")
            .expect("the banner names the bound port");
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGINT,
        )
        .expect("interrupt");
        // A forced exit returns the interrupt error code. A successful exit
        // proves that the first interrupt completed the ordinary shutdown.
        let status = child.wait().expect("host exit after its first interrupt");
        assert!(status.success(), "headless host failed: {status:?}");
        assert!(
            daemon.try_wait().expect("status").is_none(),
            "interrupt stopped the external daemon"
        );
    }));
    if let Some(mut host) = host {
        let _ = host.kill();
        let _ = host.wait();
    }
    // The host was the only writer of the pipe, so its exit ends the reader.
    if let Some(stderr_reader) = stderr_reader {
        let _ = stderr_reader.join();
    }
    let stopped = fixture
        .command(&["daemon", "stop"])
        .output()
        .expect("stop fixture daemon");
    if !stopped.status.success() {
        let _ = daemon.kill();
    }
    let _ = daemon.wait();
    daemon_output
        .join()
        .expect("external daemon reader")
        .expect("drain external daemon output");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    fixture.assert_stopped();
}

#[test]
fn quit_preserves_an_existing_daemon() {
    let fixture = Fixture::new();
    let (mut daemon, daemon_output) = fixture.external_daemon();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fixture.wait_for_daemon_address();
        let mut terminal = fixture.start(&["--port", "0"], &[]);
        terminal.until("Native sessions");
        fixture.web_port();
        terminal.until("external");
        terminal.send(b"/quit\r");
        terminal.finish();
        assert!(
            daemon.try_wait().expect("status").is_none(),
            "quit stopped the external daemon"
        );
        assert!(
            terminal.text().contains("kept running"),
            "missing external-daemon receipt: {}",
            terminal.text()
        );
    }));
    let stopped = fixture
        .command(&["daemon", "stop"])
        .output()
        .expect("stop fixture daemon");
    if !stopped.status.success() {
        let _ = daemon.kill();
    }
    let _ = daemon.wait();
    daemon_output
        .join()
        .expect("external daemon reader")
        .expect("drain external daemon output");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    fixture.assert_stopped();
}

#[test]
fn redirected_launch_without_a_subcommand_does_not_read_stdin_or_enter_the_interface() {
    let fixture = Fixture::new();
    let mut host = fixture
        .command(&["--port", "0"])
        // Let the actual Web listener choose its port; reserving one and
        // releasing it before spawning leaves a window for another socket.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("plain host");
    let stderr = fixture
        .files
        .observe_pipe(host.stderr.take().expect("host stderr"), Vec::new());
    if let Err(error) = fixture.published_web_port() {
        let _ = host.kill();
        let _ = host.wait();
        let diagnostic = stderr
            .join()
            .expect("host stderr reader")
            .expect("read host stderr");
        panic!(
            "plain host did not publish its port ({error}): {}",
            String::from_utf8_lossy(&diagnostic)
        );
    }
    let stop = fixture
        .command(&["daemon", "stop"])
        .output()
        .expect("stop plain host");
    if !stop.status.success() {
        let _ = host.kill();
    }
    let output = host.wait_with_output().expect("plain output");
    stderr
        .join()
        .expect("host stderr reader")
        .expect("read host stderr");
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(output.status.success());
    assert!(
        !output
            .stdout
            .windows(8)
            .any(|bytes| bytes == b"\x1b[?1049h")
    );
    fixture.assert_stopped();
}

#[cfg(unix)]
#[test]
fn cancelling_each_native_installer_reaps_it_and_reports_an_incomplete_setup() {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    for (index, runtime, version) in [
        (0, "codex", "0.163.0"),
        (1, "claude", "2.1.306"),
        (2, "pi", "1.4.0"),
        (3, "opencode", "2.1.4"),
    ] {
        let fixture = Fixture::new();
        let bin = fixture.root.path().join("bin");
        std::fs::create_dir(&bin).expect("fixture bin");
        let user_home = fixture.root.path().join("user");
        std::fs::create_dir_all(&user_home).expect("fixture user home");
        let control = user_home.join("install-control");
        nix::unistd::mkfifo(
            &control,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .expect("installer control FIFO");
        // Keep the installer in its read until cancellation. Dropping the
        // last writer also releases it if the test unwinds.
        let opening_reader = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(&control)
            .expect("open installer control for the writer");
        let _release_installer = OpenOptions::new()
            .write(true)
            .open(&control)
            .expect("hold installer control");
        drop(opening_reader);
        let provider = bin.join(runtime);
        std::fs::write(
            &provider,
            format!(
                r#"#!/bin/sh
if [ "$1" = --version ]; then echo '{version}'; exit 0; fi
if [ -f "$HOME/fail-after-cancel" ]; then exit 91; fi
if [ "$1" = features ]; then echo 'daemon_auto_start stable true'; exit 0; fi
if [ "$1 $2" = 'mcp list' ] || [ "$1" = plugin ]; then echo '[]'; exit 0; fi
if [ "$1" = debug ] && [ ! -f "$HOME/paths-checked" ]; then
    : > "$HOME/paths-checked"
    printf 'config %s/opencode\n' "$XDG_CONFIG_HOME"
    exit 0
fi
exec 3<"$HOME/install-control"
printf '%s' "$$" > "$HOME/install-pid.tmp"
/bin/mv "$HOME/install-pid.tmp" "$HOME/install-pid"
IFS= read -r _ <&3
"#
            ),
        )
        .expect("fake provider");
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700))
            .expect("provider mode");
        let mut terminal =
            fixture.start(&["--port", "0"], &[("PATH", bin.to_str().expect("PATH"))]);
        terminal.until("Native sessions");
        fixture.web_port();
        terminal.send(b"/setup\r");
        terminal.until("needs setup or update");
        for _ in 0..index {
            terminal.send(b"\x1b[B");
        }
        terminal.send(b" ");
        for _ in 0..4 {
            terminal.send(b"\x1b[B");
        }
        let before = terminal.mark();
        terminal.send(b"\r\r");
        terminal.until_screen_after("PgUp/PgDn   Enter select", before);
        terminal.until_screen("> Continue");
        let pid_file = fixture.root.path().join("user/install-pid");
        assert!(
            !pid_file.exists(),
            "double Enter must not start the installer"
        );
        terminal.send(b"\r");
        terminal.until_screen("PgUp/PgDn   Enter select");
        terminal.send(b"\x1b[B\r");
        let published = fixture
            .files
            .read(&pid_file)
            .expect("installer published its PID");
        let pid: i32 = std::str::from_utf8(&published)
            .expect("provider PID text")
            .parse()
            .expect("PID");
        terminal.send(b"b");
        terminal.until("interrupted");
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH),
            "{runtime} installer survived cancellation"
        );
        terminal.send(b"\r/quit\r");
        terminal.finish();
        fixture.assert_stopped();
        // A retry that encounters a real provider failure must remain an
        // explicit failure; cancellation is not a promise of automatic repair.
        std::fs::write(fixture.root.path().join("user/fail-after-cancel"), "")
            .expect("retry failure fixture");
        let retried = fixture
            .command(&["sessions", "enable", "--runtime", runtime])
            .env("PATH", &bin)
            .output()
            .expect("retry native setup");
        assert!(
            !retried.status.success(),
            "{runtime} retry concealed the provider failure"
        );
        assert!(!String::from_utf8_lossy(&retried.stdout).contains("\"enabled\": true"));
    }
}

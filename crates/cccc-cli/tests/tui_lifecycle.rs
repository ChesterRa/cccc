//! Exercise the real CLI through an OS terminal, including Windows ConPTY.
//! All state and provider homes are disposable; no provider is invoked.
use fs2::FileExt;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(45);

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    port: u16,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated root");
        let home = root.path().join("cccc");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let port = listener.local_addr().expect("address").port();
        std::fs::create_dir_all(&home).expect("home");
        Self { root, home, port }
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
        let original_modes = format!("{:?}", terminal_modes(pair.master.as_ref()));
        let child = pair.slave.spawn_command(command).expect("start CCCC");
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().expect("terminal reader");
        let writer = pair.master.take_writer().expect("terminal writer");
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                capture
                    .lock()
                    .expect("output")
                    .extend_from_slice(&buffer[..count]);
            }
        });
        Terminal {
            child,
            master: pair.master,
            writer,
            output,
            original_modes,
        }
    }

    fn assert_stopped(&self) {
        for filename in ["ccccd.lock", "cccc-web.lock"] {
            assert_lock_free(&self.home.join("daemon").join(filename));
        }
        assert!(
            !self.home.join("daemon/ccccd.addr.json").exists(),
            "daemon descriptor remained"
        );
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", self.port)).is_err(),
            "Web still listening"
        );
    }
}

struct Terminal {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Arc<Mutex<Vec<u8>>>,
    original_modes: String,
}

impl Terminal {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().expect("output")).into_owned()
    }

    fn until(&mut self, text: &str) {
        self.until_after(text, 0);
    }

    fn until_after(&mut self, text: &str, offset: usize) {
        let deadline = Instant::now() + WAIT;
        loop {
            if self
                .text()
                .get(offset..)
                .is_some_and(|output| output.contains(text))
            {
                return;
            }
            assert!(
                self.child.try_wait().expect("child status").is_none(),
                "CCCC exited before {text:?}: {}",
                self.text()
            );
            assert!(
                Instant::now() < deadline,
                "CCCC did not show {text:?}: {}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn until_screen(&mut self, text: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            // Ratatui writes only changed cells; a visible sentence need not
            // occur contiguously anywhere in the raw terminal byte stream.
            let mut screen = retach::screen::Screen::new(100, 24, 0);
            screen.process(&self.output.lock().expect("terminal output"));
            let visible = screen
                .visible_rows()
                .map(|row| row.iter().map(|cell| cell.c).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            if visible.contains(text) {
                return;
            }
            assert!(
                self.child.try_wait().expect("child status").is_none(),
                "CCCC exited before {text:?}: {visible}"
            );
            assert!(
                Instant::now() < deadline,
                "CCCC did not display {text:?}: {visible}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("terminal input");
        self.writer.flush().expect("flush");
    }

    fn finish(&mut self) {
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("child status") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "CCCC did not exit: {}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            status.success(),
            "CCCC failed ({status:?}): {}",
            self.text()
        );
        // Receipt output is written before process exit; give the reader its turn.
        let receipt_deadline = Instant::now() + Duration::from_secs(2);
        while !self.text().contains("CCCC stopped.") && Instant::now() < receipt_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            format!("{:?}", terminal_modes(self.master.as_ref())),
            self.original_modes,
            "terminal input flags were not restored"
        );
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(unix)]
fn terminal_modes(master: &dyn MasterPty) -> Option<String> {
    master.get_termios().map(|modes| format!("{modes:?}"))
}

#[cfg(not(unix))]
fn terminal_modes(_master: &dyn MasterPty) -> Option<String> {
    None
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
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.send(b"/groups\r");
    terminal.until_screen("Tasks: 25 active  1 planned  1 done");
    terminal.send(b"t");
    terminal.until_screen("Active task 25");
    terminal.send(b"\r");
    terminal.until_screen("Checklist 1/2 done");
    terminal.send(b"\x1b[F");
    terminal.until_screen("LAST_NOTES_LINE");
    terminal.send(b"bd");
    terminal.until_screen("Completed task");
    terminal.send(b"\r");
    terminal.until_screen("FINAL_COMPLETED_OUTCOME");
    terminal.send(b"bd");
    terminal.until_screen("Active task 25");
    terminal.send(b"\x1b[6~");
    terminal.until_screen("Page 2");
    terminal.until_screen("Active task 05");
    terminal.send(b"bb/quit\r");
    terminal.finish();
    fixture.assert_stopped();
}

#[cfg(target_os = "linux")]
#[test]
fn blocked_terminal_output_still_restores_input_when_closing_times_out() {
    use nix::pty::{Winsize, openpty};
    use nix::sys::signal::{Signal, kill};
    use nix::sys::termios::{FlowArg, LocalFlags, tcflow, tcgetattr};
    use nix::unistd::Pid;

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
    let mut child = fixture
        .command(&[])
        .stdin(Stdio::from(pair.slave.try_clone().expect("stdin")))
        .stdout(Stdio::from(pair.slave.try_clone().expect("stdout")))
        .stderr(Stdio::piped())
        .spawn()
        .expect("isolated host");
    let output = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut reader = std::fs::File::from(pair.master);
        let mut buffer = [0; 8192];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            capture
                .lock()
                .expect("capture")
                .extend_from_slice(&buffer[..count]);
        }
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + WAIT;
        while !String::from_utf8_lossy(&output.lock().expect("output")).contains("Native sessions")
        {
            assert!(child.try_wait().expect("host status").is_none());
            assert!(Instant::now() < deadline, "interface did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !tcgetattr(&pair.slave)
                .expect("raw mode")
                .local_flags
                .contains(LocalFlags::ICANON)
        );
        tcflow(&pair.slave, FlowArg::TCOOFF).expect("pause terminal output");
        // Let the drawing thread block in its next output write. The host's
        // shutdown must still restore input without waiting for that thread.
        std::thread::sleep(Duration::from_secs(2));
        kill(
            Pid::from_raw(child.id().try_into().expect("PID")),
            Signal::SIGTERM,
        )
        .expect("terminate only fixture host");
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().expect("host status") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "blocked terminal stalled shutdown"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
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
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
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
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.send(b"/groups\r");
    terminal.until_screen("s start");
    terminal.until_screen(&group.group_id);
    terminal.send(b"s");
    terminal.until_screen(&format!("Start {title}?"));
    terminal.send(b"\r");
    terminal.until_screen(&format!("{title}: start done."));
    let started = store.load(&group.group_id).expect("started Group");
    assert!(started.running, "an empty Group still records Start intent");
    assert_eq!(started.state, GroupState::Active);

    // Observed running is still false: Stop must remain an explicit action.
    terminal.send(b"x");
    terminal.until_screen(&format!("Stop {title}?"));
    terminal.send(b"\r");
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
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.send(b"/groups\r");
    terminal.until_screen("x stop");
    terminal.until_screen(&group.group_id);
    terminal.send(b"x");
    terminal.until_screen(&format!("Stop {title}?"));
    terminal.send(b"b");
    terminal.until_screen("x stop");
    assert_eq!(
        store.load(&group.group_id).expect("still paused").state,
        GroupState::Paused
    );
    assert!(
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("no cancelled mutation")
            .is_empty()
    );
    terminal.send(b"x\r");
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

#[test]
fn failed_web_start_reports_failure_after_stopping_its_daemon() {
    let fixture = Fixture::new();
    let occupied = std::net::TcpListener::bind(("127.0.0.1", fixture.port)).expect("occupy port");
    let mut terminal = fixture.start(&[], &[]);
    let deadline = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = terminal.child.try_wait().expect("host status") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "failed Web host did not exit: {}",
            terminal.text()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !terminal.text().contains("CCCC stopped with problems.") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        terminal.text().contains("CCCC stopped with problems."),
        "{}",
        terminal.text()
    );
    assert!(terminal.text().contains("failed:"));
    assert_eq!(
        format!("{:?}", terminal_modes(terminal.master.as_ref())),
        terminal.original_modes
    );
    drop(occupied);
    fixture.assert_stopped();
}

#[test]
fn raw_ctrl_c_stops_without_an_extra_enter() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.send(b"\x03");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn redraw_needs_no_cursor_reply_and_keeps_the_host_running() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    let before = terminal.text().len();
    terminal.send(b"\x0c");
    // This is a full repaint, so the heading must be written again. The
    // terminal deliberately never responds to cursor-position queries.
    terminal.until_after("Native sessions", before);
    assert!(terminal.child.try_wait().expect("host status").is_none());
    assert!(
        !terminal.text()[before..].contains("\x1b[6n"),
        "repainting must not request the cursor position"
    );
    terminal.send(b"/quit\r");
    terminal.finish();
    fixture.assert_stopped();
}

#[cfg(unix)]
#[test]
fn hangup_uses_the_same_graceful_cleanup() {
    let fixture = Fixture::new();
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(terminal.child.process_id().expect("host PID") as i32),
        nix::sys::signal::Signal::SIGHUP,
    )
    .expect("hangup");
    terminal.finish();
    fixture.assert_stopped();
}

#[test]
fn explicit_launches_and_readonly_mode_keep_plain_output() {
    for (args, extra) in [
        (vec!["web"], vec![]),
        (vec!["--headless", "--port", "0"], vec![]),
        (vec![], vec![("CCCC_WEB_MODE", " ro ")]),
    ] {
        let fixture = Fixture::new();
        let mut terminal = fixture.start(&args, &extra);
        terminal.until("Local:");
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
    terminal.until_screen("http://127.0.0.1:");
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
    let mut terminal = fixture.start(&[], &[]);
    terminal.until("Native sessions");
    terminal.send(b"/groups\r");
    terminal.until_screen(&group.group_id);
    terminal.send(b"x");
    terminal.until_screen("Stop Small confirmation fixture?");
    terminal
        .master
        .resize(PtySize {
            rows: 10,
            cols: 45,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("small window");
    terminal.send(b"\x0c");
    terminal.until_screen("Terminal too small");
    terminal.send(b"\r/exit\r");
    // Allow ten input polls before exiting. After shutdown, the persisted
    // lifecycle must still prove that no hidden confirmation was accepted.
    std::thread::sleep(Duration::from_secs(1));
    terminal.send(&[3]);
    terminal.finish();
    assert_eq!(
        store.load(&group.group_id).expect("untouched Group").state,
        GroupState::Active
    );
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
    let mut terminal = fixture.start(&["--headless"], &[]);
    terminal.until("Local:");
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

#[test]
fn quit_preserves_an_existing_daemon() {
    let fixture = Fixture::new();
    let mut daemon = fixture
        .command(&["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("isolated external daemon");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = Instant::now() + WAIT;
        while !fixture.home.join("daemon/ccccd.addr.json").exists() {
            assert!(Instant::now() < deadline, "external daemon not ready");
            assert!(
                daemon.try_wait().expect("status").is_none(),
                "external daemon failed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut terminal = fixture.start(&[], &[]);
        terminal.until("Native sessions");
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
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    fixture.assert_stopped();
}

#[test]
fn redirected_bare_launch_does_not_read_stdin_or_enter_the_interface() {
    let fixture = Fixture::new();
    let mut host = fixture
        .command(&[])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("plain host");
    let deadline = Instant::now() + WAIT;
    while !fixture.home.join("daemon/web_runtime.json").exists() {
        if Instant::now() >= deadline || host.try_wait().expect("status").is_some() {
            let _ = host.kill();
            let output = host.wait_with_output().expect("output");
            panic!(
                "plain host not ready: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let stop = fixture
        .command(&["daemon", "stop"])
        .output()
        .expect("stop plain host");
    if !stop.status.success() {
        let _ = host.kill();
    }
    let output = host.wait_with_output().expect("plain output");
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
    use std::os::unix::fs::PermissionsExt;
    for (index, runtime, version) in [
        (0, "codex", "0.163.0"),
        (1, "claude", "2.1.306"),
        (2, "pi", "1.4.0"),
        (3, "opencode", "2.1.4"),
    ] {
        let fixture = Fixture::new();
        let bin = fixture.root.path().join("bin");
        std::fs::create_dir(&bin).expect("fixture bin");
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
printf '%s' "$$" > "$HOME/install-pid"
exec /bin/sleep 60
"#
            ),
        )
        .expect("fake provider");
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o700))
            .expect("provider mode");
        let mut terminal = fixture.start(&[], &[("PATH", bin.to_str().expect("PATH"))]);
        terminal.until("Native sessions");
        terminal.send(b"/setup\r");
        terminal.until("needs setup or update");
        for _ in 0..index {
            terminal.send(b"\x1b[B");
        }
        terminal.send(b" \r");
        terminal.until("Enter connect");
        terminal.send(b"\r");
        let pid_file = fixture.root.path().join("user/install-pid");
        let deadline = Instant::now() + WAIT;
        while !pid_file.exists() {
            assert!(
                Instant::now() < deadline,
                "{runtime} installer did not start: {}",
                terminal.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .expect("provider PID")
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

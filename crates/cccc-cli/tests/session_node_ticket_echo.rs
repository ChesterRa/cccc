//! A pairing ticket pasted at the interactive prompt must not be echoed into the terminal.
#![cfg(unix)]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SECRET: &str = "cccc-session-node:EchoFixtureSecret";

fn read_until(
    events: &mpsc::Receiver<Vec<u8>>,
    output: &mut Vec<u8>,
    done: impl Fn(&[u8]) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(output) {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        match events.recv_timeout(left) {
            Ok(bytes) => output.extend_from_slice(&bytes),
            Err(_) => return done(output),
        }
    }
    true
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

struct Prompt {
    _temp: tempfile::TempDir,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    events: mpsc::Receiver<Vec<u8>>,
    output: Vec<u8>,
    before: String,
}

fn open_prompt() -> Prompt {
    let temp = tempfile::tempdir().expect("temporary home");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");
    let before = format!("{:?}", pair.master.get_termios().expect("terminal modes"));
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_cccc"));
    command.args(["sessions", "node", "connect", "--ticket-stdin"]);
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin");
    command.env("HOME", temp.path());
    command.env("CCCC_HOME", temp.path().join("cccc"));
    command.env("TERM", "xterm");
    command.cwd(temp.path());
    let child = pair.slave.spawn_command(command).expect("start CLI");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let (sender, events) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    let writer = pair.master.take_writer().expect("pty writer");
    let mut prompt = Prompt {
        _temp: temp,
        master: pair.master,
        child,
        writer,
        events,
        output: Vec::new(),
        before,
    };
    assert!(
        read_until(&prompt.events, &mut prompt.output, |seen| contains(
            seen,
            "press Enter"
        )),
        "prompt never appeared: {}",
        String::from_utf8_lossy(&prompt.output)
    );
    prompt
}

impl Prompt {
    fn type_bytes(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("type into terminal");
        self.writer.flush().expect("flush terminal input");
    }

    fn expect_output(&mut self, text: &str) {
        assert!(
            read_until(&self.events, &mut self.output, |seen| contains(seen, text)),
            "{text:?} never appeared: {}",
            String::from_utf8_lossy(&self.output)
        );
    }

    fn finish(mut self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("CLI status") {
                break status;
            }
            assert!(Instant::now() < deadline, "CLI did not exit");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(!status.success());
        let after = format!("{:?}", self.master.get_termios().expect("terminal modes"));
        assert_eq!(after, self.before, "terminal mode was not restored");
        self.output
    }
}

#[test]
fn interactive_ticket_is_not_echoed_and_terminal_mode_is_restored() {
    let mut prompt = open_prompt();
    prompt.type_bytes(format!("{SECRET}\r").as_bytes());
    prompt.expect_output("Invalid or missing session-node pairing ticket");
    let output = prompt.finish();
    assert!(
        !contains(&output, SECRET) && !contains(&output, "EchoFixtureSecret"),
        "ticket was echoed: {}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
fn interrupt_at_the_ticket_prompt_restores_the_terminal_and_exits() {
    let mut prompt = open_prompt();
    prompt.type_bytes(b"cccc-session-node:partial\x03");
    prompt.expect_output("Pairing cancelled");
    let output = prompt.finish();
    assert!(
        !contains(&output, "partial"),
        "{}",
        String::from_utf8_lossy(&output)
    );
}

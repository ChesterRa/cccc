//! A real PTY, driven by output, end-of-stream and the owned process's exit.
#[path = "terminal_process.rs"]
mod process;

use portable_pty::{CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize};
use retach::screen::Screen;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::SystemTime;

enum Event {
    Bytes(Vec<u8>),
    End(io::Result<()>),
    Exit(io::Result<ExitStatus>),
}

pub(super) struct Terminal {
    process: Option<process::Process>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    reader: Option<std::thread::JoinHandle<()>>,
    events: Receiver<Event>,
    output: Vec<u8>,
    eof: bool,
    status: Option<ExitStatus>,
    original_modes: Option<String>,
    restored_modes: Option<String>,
    home: PathBuf,
    launched: SystemTime,
    #[cfg(unix)]
    pub(super) pid: u32,
}

#[cfg(unix)]
fn modes(master: &dyn MasterPty) -> Option<String> {
    master.get_termios().map(|modes| format!("{modes:?}"))
}

#[cfg(not(unix))]
fn modes(_master: &dyn MasterPty) -> Option<String> {
    None
}

impl Terminal {
    pub(super) fn start(
        pair: PtyPair,
        command: CommandBuilder,
        home: PathBuf,
        on_exit: impl FnOnce(Option<&io::Error>) + Send + 'static,
    ) -> Self {
        let master = pair.master;
        let original_modes = modes(master.as_ref());
        let launched = SystemTime::now();
        let (sender, events) = mpsc::channel();
        let mut read = master.try_clone_reader().expect("terminal reader");
        let writer = master.take_writer().expect("terminal writer");
        let output = sender.clone();
        let (ready, reader_ready) = mpsc::sync_channel(0);
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            if ready.send(()).is_err() {
                return;
            }
            let end = loop {
                match read.read(&mut buffer) {
                    Ok(0) => break Ok(()),
                    Ok(count) => {
                        if output.send(Event::Bytes(buffer[..count].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => break Ok(()),
                    Err(error) => break Err(error),
                }
            };
            let _ = output.send(Event::End(end));
        });
        // Arm the reader before a short-lived child can print and close the last slave.
        reader_ready.recv().expect("terminal reader ready");
        let child = pair.slave.spawn_command(command).expect("start CCCC");
        drop(pair.slave);
        #[cfg(unix)]
        let pid = child.process_id().expect("host PID");
        let process = process::Process::watch(child, move |status| {
            on_exit(status.as_ref().err());
            let _ = sender.send(Event::Exit(status));
        });
        Self {
            process: Some(process),
            master: Some(master),
            writer: Some(writer),
            reader: Some(reader),
            events,
            output: Vec::new(),
            eof: false,
            status: None,
            original_modes,
            restored_modes: None,
            home,
            launched,
            #[cfg(unix)]
            pid,
        }
    }

    fn record(&mut self, event: Event) {
        match event {
            Event::Bytes(bytes) => self.output.extend(bytes),
            Event::End(result) => {
                self.eof = true;
                result.expect("read PTY output");
            }
            Event::Exit(result) => {
                self.status = Some(result.expect("wait for CCCC"));
                self.restored_modes = modes(self.master.as_deref().expect("open terminal"));
                #[cfg(not(unix))]
                {
                    // Closing ConPTY can flush the final repaint before its pipe reaches EOF.
                    // Its independent reader keeps draining while the console handle closes.
                    self.writer.take();
                    self.master.take();
                }
            }
        }
    }

    fn receive(&mut self) {
        let event = self
            .events
            .recv()
            .expect("terminal observers closed unexpectedly");
        self.record(event);
    }

    pub(super) fn mark(&mut self) -> usize {
        while let Ok(event) = self.events.try_recv() {
            self.record(event);
        }
        self.output.len()
    }

    fn more(&mut self, expected: &str) {
        let before = self.output.len();
        while self.output.len() == before {
            assert!(
                !self.eof,
                "PTY output ended before {expected:?} (exit {:?}): {}{}",
                self.status,
                String::from_utf8_lossy(&self.output),
                self.startup_state()
            );
            self.receive();
        }
        self.mark();
    }

    pub(super) fn text(&mut self) -> String {
        self.mark();
        String::from_utf8_lossy(&self.output).into_owned()
    }

    pub(super) fn text_after(&self, offset: usize) -> String {
        String::from_utf8_lossy(&self.output[offset..]).into_owned()
    }

    pub(super) fn until(&mut self, text: &str) {
        self.until_after(text, 0);
    }

    pub(super) fn until_after(&mut self, text: &str, offset: usize) {
        let needle = text.as_bytes();
        assert!(!needle.is_empty());
        self.mark();
        let mut start = offset;
        loop {
            if self.output[start..]
                .windows(needle.len())
                .any(|window| window == needle)
            {
                return;
            }
            // Include the last partial match when the next PTY read arrives.
            start = offset.max(self.output.len().saturating_sub(needle.len() - 1));
            self.more(text);
        }
    }

    pub(super) fn until_screen(&mut self, text: &str) {
        let mut screen = Screen::new(100, 24, 0);
        let mut consumed = 0;
        self.mark();
        loop {
            screen.process(&self.output[consumed..]);
            consumed = self.output.len();
            let visible = screen
                .visible_rows()
                .map(|row| row.iter().map(|cell| cell.c).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            if visible.contains(text) {
                return;
            }
            self.more(text);
        }
    }

    pub(super) fn until_screen_after(&mut self, text: &str, offset: usize) {
        self.mark();
        let needle: Vec<char> = text.chars().collect();
        assert!(!needle.is_empty());
        let contains = |screen: &Screen| {
            screen.visible_rows().any(|row| {
                row.windows(needle.len()).any(|cells| {
                    cells
                        .iter()
                        .zip(&needle)
                        .all(|(cell, expected)| cell.c == *expected)
                })
            })
        };
        let mut screen = Screen::new(100, 24, 0);
        screen.process(&self.output[..offset]);
        assert!(
            !contains(&screen),
            "{text:?} was already visible before input"
        );
        let mut consumed = offset;
        loop {
            // A later repaint in this same read may already erase the confirmation.
            for byte in &self.output[consumed..] {
                screen.process(std::slice::from_ref(byte));
                if contains(&screen) {
                    return;
                }
            }
            consumed = self.output.len();
            self.more(text);
        }
    }

    pub(super) fn confirm_then(&mut self, open: &[u8], confirmation: &str, keys: &[u8]) {
        let before = self.mark();
        if cfg!(windows) {
            // ConPTY can coalesce an intermediate repaint: actually observe it before dismissal.
            self.send(open);
            self.until_screen_after(confirmation, before);
            self.send(keys);
        } else {
            self.send(&[open, keys].concat());
            self.until_screen_after(confirmation, before);
        }
    }

    pub(super) fn send(&mut self, bytes: &[u8]) {
        let writer = self.writer.as_mut().expect("input after host exit");
        writer.write_all(bytes).expect("terminal input");
        writer.flush().expect("flush");
    }

    pub(super) fn resize(&self, size: PtySize) {
        self.master
            .as_ref()
            .expect("terminal already closed")
            .resize(size)
            .expect("resize terminal");
    }

    pub(super) fn finish_output(&mut self) -> ExitStatus {
        while self.status.is_none() || !self.eof {
            self.receive();
        }
        assert_eq!(
            self.restored_modes, self.original_modes,
            "terminal input flags were not restored"
        );
        self.status.as_ref().expect("exit").clone()
    }

    pub(super) fn finish(&mut self) {
        let status = self.finish_output();
        assert!(
            status.success(),
            "CCCC failed ({status:?}): {}",
            self.text()
        );
    }

    /// The daemon files in the isolated Home, each with its size and when it
    /// last changed relative to launch, and the end of the content-free
    /// diagnostics log, so that a stall seen only on a CI runner names the
    /// stage it stopped in. Paths and the daemon's own output stay out of
    /// the test output.
    fn startup_state(&self) -> String {
        let since = |time: SystemTime| match time.duration_since(self.launched) {
            Ok(after) => format!("+{after:.1?}"),
            Err(_) => "before launch".into(),
        };
        let daemon = self.home.join("daemon");
        let mut report = format!("\n--- daemon files at {}:", since(SystemTime::now()));
        let mut entries: Vec<_> = std::fs::read_dir(&daemon)
            .map(|entries| entries.flatten().collect())
            .unwrap_or_default();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            if let Ok(metadata) = entry.metadata() {
                let changed = metadata.modified().map(since).unwrap_or_default();
                report.push_str(&format!(
                    "\n  {} {} bytes, changed {changed}",
                    entry.file_name().to_string_lossy(),
                    metadata.len()
                ));
            }
        }
        if let Ok(mut log) = std::fs::File::open(daemon.join("ccccd-diagnostics.log")) {
            use std::io::{Seek, SeekFrom};
            let length = log.seek(SeekFrom::End(0)).unwrap_or(0);
            let mut tail = Vec::new();
            if log
                .seek(SeekFrom::Start(length.saturating_sub(2000)))
                .is_ok()
                && log.take(2000).read_to_end(&mut tail).is_ok()
            {
                report.push_str(&format!(
                    "\n--- end of ccccd-diagnostics.log:\n{}",
                    String::from_utf8_lossy(&tail)
                ));
            }
        }
        report
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        drop(self.process.take());
        self.writer.take();
        self.master.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

use crate::RuntimeError;
use crate::output::{HistoryPage, OutputBuffer};
use cccc_contracts::{RunnerKind, utc_now};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub group_id: String,
    pub actor_id: String,
    pub runner: RunnerKind,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub group_id: String,
    pub actor_id: String,
    pub runner: RunnerKind,
    pub running: bool,
    pub pid: Option<u32>,
    pub started_at: String,
    pub exit_code: Option<u32>,
}

pub struct Session {
    status: SessionStatus,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    input_gate: Arc<Mutex<()>>,
    output: Arc<Mutex<OutputBuffer>>,
}

impl Session {
    pub fn start(spec: LaunchSpec) -> Result<Self, RuntimeError> {
        let (program, args) = spec
            .command
            .split_first()
            .ok_or(RuntimeError::EmptyCommand)?;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: spec.rows.max(1),
                cols: spec.cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(spec.cwd);
        for (key, value) in spec.env {
            command.env(key, value);
        }
        command.env("CCCC_GROUP_ID", &spec.group_id);
        command.env("CCCC_ACTOR_ID", &spec.actor_id);
        command.env("CCCC_RUNNER", runner_name(spec.runner));
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let pid = child.process_id();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let output = Arc::new(Mutex::new(OutputBuffer::default()));
        let target = Arc::clone(&output);
        std::thread::Builder::new()
            .name(format!("cccc-runtime:{}:{}", spec.group_id, spec.actor_id))
            .spawn(move || copy_output(&mut reader, &target))?;
        Ok(Self {
            status: SessionStatus {
                group_id: spec.group_id,
                actor_id: spec.actor_id,
                runner: spec.runner,
                running: true,
                pid,
                started_at: utc_now(),
                exit_code: None,
            },
            master: pair.master,
            child,
            writer,
            input_gate: Arc::new(Mutex::new(())),
            output,
        })
    }

    pub fn status(&mut self) -> SessionStatus {
        if self.status.running
            && let Ok(Some(exit)) = self.child.try_wait()
        {
            self.status.running = false;
            self.status.exit_code = Some(exit.exit_code());
        }
        self.status.clone()
    }

    pub fn stop(&mut self) -> Result<SessionStatus, RuntimeError> {
        if self.status.running {
            self.child
                .kill()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let exit = self
                .child
                .wait()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            self.status.running = false;
            self.status.exit_code = Some(exit.exit_code());
        }
        Ok(self.status.clone())
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), RuntimeError> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    pub(crate) fn input_gate(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.input_gate)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), RuntimeError> {
        self.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| RuntimeError::Io(std::io::Error::other(error.to_string())))
    }

    pub fn history(&self, before: Option<u64>, limit: usize) -> Result<HistoryPage, RuntimeError> {
        self.output
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|output| output.page(before, limit))
    }

    pub fn history_since(&self, after: u64, limit: usize) -> Result<HistoryPage, RuntimeError> {
        self.output
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|output| output.page_since(after, limit))
    }

    pub fn clear(&self) -> Result<(), RuntimeError> {
        self.output
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .clear();
        Ok(())
    }

    pub fn bracketed_paste_enabled(&self) -> Result<bool, RuntimeError> {
        self.output
            .lock()
            .map_err(|_| RuntimeError::Poisoned)
            .map(|output| output.bracketed_paste_enabled())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.status.running {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.status.running = false;
        }
    }
}

fn copy_output(reader: &mut dyn Read, output: &Arc<Mutex<OutputBuffer>>) {
    let mut buffer = [0_u8; 8192];
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let Ok(mut target) = output.lock() else {
            break;
        };
        target.push(&buffer[..count]);
    }
}

fn runner_name(runner: RunnerKind) -> &'static str {
    match runner {
        RunnerKind::Pty => "pty",
        RunnerKind::Headless => "headless",
    }
}

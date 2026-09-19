//! Host-owned interactive commands reuse the PTY implementation without joining
//! the Actor registry or its completed-history cache/reaper.
use crate::session::Session;
use crate::{HistoryPage, LaunchSpec, RuntimeError, SessionStatus};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct CommandSession(Arc<Mutex<Session>>);

impl CommandSession {
    pub fn start(spec: LaunchSpec) -> Result<Self, RuntimeError> {
        Ok(Self(Arc::new(Mutex::new(Session::start_with_history(
            spec, None, 0,
        )?))))
    }

    /// Drain the reader before reporting completion, including the final bytes.
    pub fn status(&self) -> Result<SessionStatus, RuntimeError> {
        let mut session = self.0.lock().map_err(|_| RuntimeError::Poisoned)?;
        let status = session.status();
        if !status.running {
            session.finish_output()?;
        }
        Ok(status)
    }

    pub fn stop(&self) -> Result<SessionStatus, RuntimeError> {
        let mut session = self.0.lock().map_err(|_| RuntimeError::Poisoned)?;
        // Preserve a natural exit code if the process has already finished.
        session.status();
        session.stop()
    }

    pub fn history_since(&self, cursor: u64, limit: usize) -> Result<HistoryPage, RuntimeError> {
        let mut session = self.0.lock().map_err(|_| RuntimeError::Poisoned)?;
        let page = session.history_since(cursor, limit)?;
        if page.data.is_empty() && page.has_more && !session.status().running {
            // A process may end halfway through a UTF-8 character. No next
            // byte will arrive; expose replacement text rather than wait forever.
            let end = session.history_handle().end_cursor()?;
            let mut tail = session.history(
                Some((page.start_cursor.saturating_add(limit.max(1) as u64)).min(end)),
                limit,
            )?;
            tail.has_more = tail.end_cursor < end;
            return Ok(tail);
        }
        Ok(page)
    }

    pub fn write(&self, data: &[u8]) -> Result<(), RuntimeError> {
        let gate = self
            .0
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .input_gate();
        let _input = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
        let writer = self
            .0
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .input_writer()?;
        // Never hold the lifecycle lock across input: timeout/stop must be able
        // to terminate a child that is not reading its PTY.
        crate::pty_input::write_input(&writer, data)
    }
}

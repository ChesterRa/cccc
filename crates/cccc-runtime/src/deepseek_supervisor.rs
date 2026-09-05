//! Small, bounded ACP process supervisor used by DeepSeek actor adapters.
//! Durable output/cursor writes intentionally live above this layer.
mod lifecycle;
use crate::deepseek_acp::MAX_PENDING_REQUESTS;
use crate::deepseek_acp::{self, NdjsonSession, ProtocolError};
use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};
use std::process::Child;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const STDERR_TAIL_BYTES: usize = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("deepseek command is empty")]
    EmptyCommand,
    #[error("deepseek prompt queue is full")]
    QueueFull,
    #[error("deepseek already has an active prompt")]
    PromptActive,
    #[error("deepseek supervisor is not running")]
    NotRunning,
    #[error("deepseek process I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("deepseek ACP protocol failed: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("deepseek ACP process ended before responding")]
    Eof,
    #[error("deepseek ACP response timed out")]
    Timeout,
    #[error("deepseek ACP response id did not match the request")]
    UnexpectedResponse,
}

#[derive(Debug)]
pub struct DeepSeekSupervisor {
    child: Option<Child>,
    process_tree: Option<crate::OwnedProcessTree>,
    queue: VecDeque<(u64, String)>,
    generation: u64,
    next_request_id: u64,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    stdout_rx: Option<Receiver<Option<Vec<u8>>>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    protocol: NdjsonSession,
    session_id: Option<String>,
    active_request_id: Option<u64>,
    pending_permissions: HashSet<String>,
}

impl Default for DeepSeekSupervisor {
    fn default() -> Self {
        Self {
            child: None,
            process_tree: None,
            queue: VecDeque::with_capacity(MAX_PENDING_REQUESTS),
            generation: 0,
            next_request_id: 1,
            stderr_tail: Arc::new(Mutex::new(Vec::new())),
            stdout_rx: None,
            stdout_thread: None,
            stderr_thread: None,
            protocol: NdjsonSession::default(),
            session_id: None,
            active_request_id: None,
            pending_permissions: HashSet::new(),
        }
    }
}

impl DeepSeekSupervisor {
    pub fn enqueue(&mut self, prompt: impl Into<String>) -> Result<u64, SupervisorError> {
        let child = self.child.as_ref().ok_or(SupervisorError::NotRunning)?;
        if child.id() == 0 {
            return Err(SupervisorError::NotRunning);
        }
        if self.queue.len() >= MAX_PENDING_REQUESTS {
            return Err(SupervisorError::QueueFull);
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.queue.push_back((request_id, prompt.into()));
        Ok(request_id)
    }

    /// Write at most one queued prompt.  A caller drives this after session/new
    /// and records the request's generation before it can affect durable state.
    pub fn flush_one(&mut self, session_id: &str) -> Result<Option<u64>, SupervisorError> {
        let (request_id, prompt) = match self.queue.pop_front() {
            Some(turn) => turn,
            None => return Ok(None),
        };
        if self.active_request_id.is_some() {
            self.queue.push_front((request_id, prompt));
            return Err(SupervisorError::PromptActive);
        }
        let payload = serde_json::json!({
            "jsonrpc":"2.0", "id":request_id, "method":"session/prompt",
            "params":{"sessionId":session_id,"prompt":[{"type":"text","text":prompt}]}
        });
        let id = serde_json::json!(request_id);
        self.protocol.register(&id)?;
        if let Err(error) = self.write_frame(&payload) {
            self.protocol.discard_pending(&id);
            return Err(error);
        }
        self.active_request_id = Some(request_id);
        Ok(Some(request_id))
    }

    pub fn cancel(&mut self) -> Result<(), SupervisorError> {
        let session_id = self.session_id.clone().ok_or(SupervisorError::NotRunning)?;
        self.send_notification(&serde_json::json!({
            "jsonrpc":"2.0",
            "method":"session/cancel",
            "params":{"sessionId":session_id}
        }))
    }

    pub fn next_frame(&mut self, timeout: Duration) -> Result<serde_json::Value, SupervisorError> {
        let frame = match self
            .stdout_rx
            .as_ref()
            .ok_or(SupervisorError::NotRunning)?
            .recv_timeout(timeout)
        {
            Ok(Some(line)) => line,
            Ok(None) => return Err(SupervisorError::Eof),
            Err(RecvTimeoutError::Timeout) => return Err(SupervisorError::Timeout),
            Err(RecvTimeoutError::Disconnected) => return Err(SupervisorError::Eof),
        };
        let value = self.protocol.feed_line(frame.trim_ascii_end())?;
        if value.get("method") == Some(&serde_json::json!("session/request_permission")) {
            if let Some(id) = value.get("id") {
                self.pending_permissions.insert(id.to_string());
            }
        }
        if value.get("method").is_none()
            && self
                .active_request_id
                .is_some_and(|id| value.get("id") == Some(&serde_json::json!(id)))
        {
            self.active_request_id = None;
        }
        Ok(value)
    }

    pub fn respond_permission(
        &mut self,
        request_id: &serde_json::Value,
        options: &serde_json::Value,
        stopping: bool,
    ) -> Result<(), SupervisorError> {
        let outcome = deepseek_acp::permission_outcome(options, stopping);
        let payload = serde_json::json!({"jsonrpc":"2.0", "id":request_id, "result":outcome});
        let result = self.write_frame(&payload);
        if result.is_ok() {
            self.pending_permissions.remove(&request_id.to_string());
        }
        result
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn is_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(|child| {
            self.process_tree
                .as_ref()
                .is_some_and(|tree| tree.try_wait(|| child.try_wait()).ok().flatten().is_none())
        })
    }

    pub fn append_stderr(&mut self, bytes: &[u8]) {
        if let Ok(mut tail) = self.stderr_tail.lock() {
            tail.extend_from_slice(bytes);
            if tail.len() > STDERR_TAIL_BYTES {
                let start = tail.len() - STDERR_TAIL_BYTES;
                tail.drain(..start);
            }
        }
    }

    pub fn stderr_tail(&self) -> Vec<u8> {
        self.stderr_tail
            .lock()
            .map(|tail| tail.clone())
            .unwrap_or_default()
    }

    fn send_request(
        &mut self,
        payload: &serde_json::Value,
        id: u64,
    ) -> Result<(), SupervisorError> {
        let id = serde_json::json!(id);
        self.protocol.register(&id)?;
        if let Err(error) = self.write_frame(payload) {
            self.protocol.discard_pending(&id);
            return Err(error);
        }
        Ok(())
    }

    fn send_notification(&mut self, payload: &serde_json::Value) -> Result<(), SupervisorError> {
        self.write_frame(payload)
    }

    fn write_frame(&mut self, payload: &serde_json::Value) -> Result<(), SupervisorError> {
        let child = self.child.as_mut().ok_or(SupervisorError::NotRunning)?;
        let stdin = child.stdin.as_mut().ok_or(SupervisorError::NotRunning)?;
        writeln!(stdin, "{payload}").map_err(SupervisorError::Io)?;
        stdin.flush().map_err(SupervisorError::Io)
    }

    fn recv_response(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<serde_json::Value, SupervisorError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SupervisorError::Timeout);
            }
            let frame = match self
                .stdout_rx
                .as_ref()
                .ok_or(SupervisorError::NotRunning)?
                .recv_timeout(remaining)
            {
                Ok(Some(line)) => line,
                Ok(None) => return Err(SupervisorError::Eof),
                Err(RecvTimeoutError::Timeout) => return Err(SupervisorError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(SupervisorError::Eof),
            };
            let value = self.protocol.feed_line(frame.trim_ascii_end())?;
            if value.get("id") == Some(&serde_json::json!(request_id)) {
                if self.active_request_id == Some(request_id) {
                    self.active_request_id = None;
                }
                return Ok(value);
            }
        }
    }
}

#[cfg(test)]
#[path = "deepseek_supervisor/tests.rs"]
mod tests;

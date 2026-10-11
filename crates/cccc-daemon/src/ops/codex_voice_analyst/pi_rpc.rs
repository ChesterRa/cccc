//! Pi's native JSONL RPC port. Command acceptance is not a model terminal.
use super::{AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

mod events;
mod framing;
mod interactions;
mod protocol_loop;
#[cfg(test)]
mod tests;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

struct Request {
    command: String,
    params: Value,
    response: oneshot::Sender<io::Result<Value>>,
}

enum Command {
    Request(Request),
    Prompt {
        session_id: String,
        delegation_id: String,
        text: String,
        response: oneshot::Sender<io::Result<String>>,
    },
    Cancel {
        session_id: String,
        delegation_id: Option<String>,
        turn_id: Option<String>,
        response: oneshot::Sender<io::Result<Option<String>>>,
    },
    Interaction {
        request_id: String,
        reply: Value,
        response: oneshot::Sender<io::Result<()>>,
    },
    Close,
}

pub(super) struct PiRpcClient {
    commands: mpsc::Sender<Command>,
    pub(super) events: broadcast::Sender<AnalystEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
    permissions: Arc<Mutex<BTreeMap<String, Value>>>,
    running: Arc<AtomicBool>,
    prompt_receipt: Arc<Mutex<Option<super::super::runtime_session::native_acp::PromptReceipt>>>,
}

impl PiRpcClient {
    pub(super) fn new(
        stdin: ChildStdin,
        stdout: ChildStdout,
        generation: String,
        session_id: String,
    ) -> io::Result<Self> {
        if generation.is_empty() || session_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Pi owner identity is missing",
            ));
        }
        let (commands, receiver) = mpsc::channel(32);
        let (frames, frame_receiver) = mpsc::channel(256);
        let (events, _) = broadcast::channel(2048);
        let permissions = Arc::new(Mutex::new(BTreeMap::new()));
        let running = Arc::new(AtomicBool::new(true));
        let prompt_receipt = Arc::new(Mutex::new(None));
        framing::spawn_reader(stdout, frames)?;
        let task = tokio::spawn(protocol_loop::run(
            Arc::new(Mutex::new(stdin)),
            receiver,
            frame_receiver,
            events.clone(),
            generation,
            session_id,
            Arc::clone(&permissions),
            Arc::clone(&running),
            Arc::clone(&prompt_receipt),
        ));
        Ok(Self {
            commands,
            events,
            task: Mutex::new(Some(task)),
            permissions,
            running,
            prompt_receipt,
        })
    }

    pub(super) fn set_prompt_receipt(
        &self,
        receipt: super::super::runtime_session::native_acp::PromptReceipt,
    ) {
        if let Ok(mut current) = self.prompt_receipt.lock() {
            *current = Some(receipt);
        }
    }

    pub(super) fn resumable(&self) -> bool {
        self.prompt_receipt
            .lock()
            .is_ok_and(|receipt| receipt.as_ref().is_some_and(|receipt| receipt.resumable()))
    }

    #[cfg(test)]
    pub(super) fn publish_for_test(&self, event: AnalystEvent) {
        let _ = self.events.send(event);
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.events.subscribe()
    }

    pub(super) fn running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) async fn request(
        &self,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(Command::Request(Request {
                command: command.into(),
                params,
                response,
            }))
            .await
            .map_err(|_| closed_error())?;
        tokio::time::timeout(timeout, receiver)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Pi RPC request timed out"))?
            .map_err(|_| closed_error())?
    }

    pub(super) async fn start_prompt(
        &self,
        session_id: &str,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<String> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(Command::Prompt {
                session_id: session_id.into(),
                delegation_id: delegation_id.into(),
                text: text.into(),
                response,
            })
            .await
            .map_err(|_| closed_error())?;
        tokio::time::timeout(REQUEST_TIMEOUT, receiver)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Pi prompt admission could not be confirmed; it was not replayed",
                )
            })?
            .map_err(|_| closed_error())?
    }

    pub(super) async fn cancel(&self, session_id: &str, turn_id: &str) -> io::Result<()> {
        self.cancel_owned(session_id, None, Some(turn_id))
            .await
            .map(|_| ())
    }

    pub(super) async fn cancel_input(
        &self,
        session_id: &str,
        delegation_id: Option<&str>,
    ) -> io::Result<Option<String>> {
        self.cancel_owned(session_id, delegation_id, None).await
    }

    async fn cancel_owned(
        &self,
        session_id: &str,
        delegation_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> io::Result<Option<String>> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(Command::Cancel {
                session_id: session_id.into(),
                delegation_id: delegation_id.map(str::to_owned),
                turn_id: turn_id.map(str::to_owned),
                response,
            })
            .await
            .map_err(|_| closed_error())?;
        tokio::time::timeout(REQUEST_TIMEOUT, receiver)
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "Pi cancellation was not confirmed")
            })?
            .map_err(|_| closed_error())?
    }

    pub(super) fn permissions(&self) -> Vec<Value> {
        self.permissions
            .lock()
            .map(|items| items.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) async fn respond_permission(&self, request_id: &str, allow: bool) -> io::Result<()> {
        self.respond_interaction(request_id, json!({"confirmed":allow}))
            .await
    }

    pub(super) async fn respond_interaction(
        &self,
        request_id: &str,
        reply: Value,
    ) -> io::Result<()> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(Command::Interaction {
                request_id: request_id.into(),
                reply,
                response,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn close(&self) {
        let _ = self.commands.send(Command::Close).await;
        let task = self.task.lock().ok().and_then(|mut task| task.take());
        if let Some(mut task) = task
            && tokio::time::timeout(STOP_TIMEOUT, &mut task).await.is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.running
            .store(false, std::sync::atomic::Ordering::Release);
        if let Ok(mut items) = self.permissions.lock() {
            items.clear();
        }
    }
}

impl Drop for PiRpcClient {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
        self.running
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

fn closed_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "managed Pi RPC session is closed",
    )
}

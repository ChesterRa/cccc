use super::{AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD};
use serde_json::Value;
use std::io;
use std::process::{ChildStdin, ChildStdout};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

mod events;
mod framing;
mod pending;
mod permissions;
mod protocol_loop;
mod tool_results;

const EVENT_CAPACITY: usize = 2048;
const COMMAND_CAPACITY: usize = 32;
const FRAME_CAPACITY: usize = 256;
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

struct RpcRequest {
    method: String,
    params: Value,
    response: oneshot::Sender<io::Result<Value>>,
}

struct PromptRequest {
    session_id: String,
    text: String,
    delegation_id: String,
    response: oneshot::Sender<io::Result<String>>,
}

enum AcpCommand {
    Request(RpcRequest),
    Prompt(PromptRequest),
    Cancel {
        session_id: String,
        response: oneshot::Sender<io::Result<()>>,
    },
    Respond {
        id: Value,
        result: Value,
    },
    RespondError {
        id: Value,
        error: Value,
    },
    ExternalStatus {
        session_id: String,
        busy: bool,
    },
    ObservedUserText {
        session_id: String,
        text: String,
    },
    ExternalDisconnected {
        reason: String,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PermissionPolicy {
    Reject,
    AllowOnce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptCompletion {
    Response,
    BoundedPostResponseDrain,
}

#[derive(Clone)]
pub(super) struct AcpLifecycleControl {
    commands: mpsc::Sender<AcpCommand>,
}

impl AcpLifecycleControl {
    pub(super) async fn status(&self, session_id: &str, busy: bool) -> io::Result<()> {
        self.commands
            .send(AcpCommand::ExternalStatus {
                session_id: session_id.to_owned(),
                busy,
            })
            .await
            .map_err(|_| closed_error())
    }

    pub(super) async fn user_text(&self, session_id: &str, text: &str) -> io::Result<()> {
        self.commands
            .send(AcpCommand::ObservedUserText {
                session_id: session_id.to_owned(),
                text: text.to_owned(),
            })
            .await
            .map_err(|_| closed_error())
    }

    pub(super) async fn disconnected(&self, reason: impl Into<String>) {
        let _ = self
            .commands
            .send(AcpCommand::ExternalDisconnected {
                reason: reason.into(),
            })
            .await;
    }
}

pub(super) struct AcpClient {
    commands: mpsc::Sender<AcpCommand>,
    pub(super) events: broadcast::Sender<AnalystEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
    auxiliary_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl AcpClient {
    pub(super) fn new(
        stdin: ChildStdin,
        stdout: ChildStdout,
        generation: String,
        runtime: &'static str,
        permission_policy: PermissionPolicy,
        prompt_completion: PromptCompletion,
    ) -> io::Result<Self> {
        let (commands, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (frames, frame_receiver) = mpsc::channel(FRAME_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        framing::spawn_reader(stdout, frames)?;
        let task = tokio::spawn(protocol_loop::run(
            std::sync::Arc::new(Mutex::new(stdin)),
            receiver,
            frame_receiver,
            events.clone(),
            generation,
            runtime,
            permission_policy,
            prompt_completion,
        ));
        Ok(Self {
            commands,
            events,
            task: Mutex::new(Some(task)),
            auxiliary_tasks: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn lifecycle_control(&self) -> AcpLifecycleControl {
        AcpLifecycleControl {
            commands: self.commands.clone(),
        }
    }

    pub(super) fn register_auxiliary_task(&self, task: JoinHandle<()>) {
        if let Ok(mut tasks) = self.auxiliary_tasks.lock() {
            tasks.push(task);
        } else {
            task.abort();
        }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.events.subscribe()
    }

    pub(super) async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::Request(RpcRequest {
                method: method.to_owned(),
                params,
                response: sender,
            }))
            .await
            .map_err(|_| closed_error())?;
        tokio::time::timeout(timeout, receiver)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("ACP request timed out: {method}"),
                )
            })?
            .map_err(|_| closed_error())?
    }

    pub(super) async fn start_prompt(
        &self,
        session_id: &str,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<String> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::Prompt(PromptRequest {
                session_id: session_id.to_owned(),
                text: text.to_owned(),
                delegation_id: delegation_id.to_owned(),
                response: sender,
            }))
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn cancel(&self, session_id: &str) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::Cancel {
                session_id: session_id.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn respond(&self, id: Value, result: Value) -> io::Result<()> {
        self.commands
            .send(AcpCommand::Respond { id, result })
            .await
            .map_err(|_| closed_error())
    }

    pub(super) async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        self.commands
            .send(AcpCommand::RespondError { id, error })
            .await
            .map_err(|_| closed_error())
    }

    pub(super) async fn close(&self) {
        if let Ok(mut tasks) = self.auxiliary_tasks.lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
        let _ = self.commands.send(AcpCommand::Close).await;
        let task = self.task.lock().ok().and_then(|mut task| task.take());
        if let Some(mut task) = task
            && tokio::time::timeout(STOP_TIMEOUT, &mut task).await.is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        if let Ok(tasks) = self.auxiliary_tasks.get_mut() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

fn closed_error() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "managed ACP session is closed")
}

use super::{AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD, kilo_input::KiloUserMessages};
use serde_json::{Value, json};
use std::io;
use std::process::{ChildStdin, ChildStdout};
use std::sync::{Mutex, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

mod diagnostic;
mod events;
mod framing;
mod interactions;
mod pending;
mod permissions;
mod protocol_loop;
mod tool_results;

const EVENT_CAPACITY: usize = 2048;
const COMMAND_CAPACITY: usize = 32;
const FRAME_CAPACITY: usize = 256;
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const LIFECYCLE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

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
        delegation_id: Option<String>,
        /// Stop's cancel: refuse every later prompt before cancelling, so a
        /// submission already on its way cannot start after it.
        close_admission: bool,
        response: oneshot::Sender<io::Result<Option<String>>>,
    },
    Respond {
        id: Value,
        result: Value,
    },
    RespondError {
        id: Value,
        error: Value,
    },
    Permission {
        request_id: String,
        reply: Value,
        response: oneshot::Sender<io::Result<()>>,
    },
    ExternalStatus {
        session_id: String,
        busy: bool,
        error: Option<Value>,
    },
    SessionUpdate {
        session_id: String,
        update: Value,
    },
    ObservedUserText {
        session_id: String,
        text: String,
        consumed: bool,
        response: oneshot::Sender<io::Result<bool>>,
    },
    RegisterNativeInput {
        delegation_id: String,
        text: String,
        response: oneshot::Sender<io::Result<()>>,
    },
    ForgetNativeInput {
        delegation_id: String,
        response: oneshot::Sender<()>,
    },
    ExternalDisconnected {
        reason: String,
    },
    /// Where one turn stands, without changing anything.
    TurnEnd {
        turn_id: String,
        response: oneshot::Sender<TurnEnd>,
    },
    Close,
}

/// Where one prompt this client wrote stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnEnd {
    /// The loop still holds it.
    Open,
    /// Its prompt answered with an explicit stopReason: the provider ended it.
    Stopped,
    /// It ended without one, by an error or a lost link, so its outcome is
    /// unknown (Hermes reports a lost turn as `unknown_execution`).
    Unknown,
}

/// What a bounded cancel observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CancelOutcome {
    /// No prompt of this client was open. A turn typed in the provider's own
    /// terminal is not one: the provider cancels only this client's prompts.
    NoTurn,
    /// The cancelled prompt answered with an explicit stopReason.
    Settled,
    /// The cancel was written, but the prompt was still open at the deadline
    /// or ended without a stopReason, so its outcome is unknown.
    Requested,
    /// The cancel was not confirmed written: the loop had ended, failed the
    /// write or did not answer in time.
    Unconfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PermissionPolicy {
    Reject,
    AllowOnce,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptCompletion {
    Response,
    /// Official AGY does not echo user text. Its first live, same-session
    /// activity is the receipt; all turn updates precede the prompt response.
    ResponseWithActivityReceipt,
    /// OpenCode/Kilo use the ordered backend stream for output and completion.
    SessionEvents,
    BoundedPostResponseDrain,
}

#[derive(Clone)]
pub(super) struct AcpLifecycleControl {
    commands: mpsc::Sender<AcpCommand>,
}

impl AcpLifecycleControl {
    #[cfg(test)]
    pub(super) async fn status(&self, session_id: &str, busy: bool) -> io::Result<()> {
        self.session_status(session_id, busy, None).await
    }

    pub(super) async fn session_status(
        &self,
        session_id: &str,
        busy: bool,
        error: Option<Value>,
    ) -> io::Result<()> {
        self.commands
            .send(AcpCommand::ExternalStatus {
                session_id: session_id.to_owned(),
                busy,
                error,
            })
            .await
            .map_err(|_| closed_error())
    }

    /// Returns true when the observed text came from the native TUI rather than
    /// echoing a CCCC-controlled ACP prompt.
    pub(super) async fn user_text(&self, session_id: &str, text: &str) -> io::Result<bool> {
        self.observe_user_text(session_id, text, true).await
    }

    pub(super) async fn observe_user_text(
        &self,
        session_id: &str,
        text: &str,
        consumed: bool,
    ) -> io::Result<bool> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::ObservedUserText {
                session_id: session_id.to_owned(),
                text: text.to_owned(),
                consumed,
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn session_update(&self, session_id: &str, update: Value) -> io::Result<()> {
        self.commands
            .send(AcpCommand::SessionUpdate {
                session_id: session_id.to_owned(),
                update,
            })
            .await
            .map_err(|_| closed_error())
    }

    pub(super) async fn set_config_option(
        &self,
        session_id: &str,
        config_id: &str,
        value: &str,
    ) -> io::Result<()> {
        send_request(
            &self.commands,
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":config_id,"value":value}),
            LIFECYCLE_REQUEST_TIMEOUT,
        )
        .await
        .map(|_| ())
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
    pub(super) kilo_input: OnceLock<Weak<KiloUserMessages>>,
    pub(super) events: broadcast::Sender<AnalystEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
    auxiliary_tasks: Mutex<Vec<JoinHandle<()>>>,
    permissions: std::sync::Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    prompt_receipt: protocol_loop::SharedPromptReceipt,
    open_prompt: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
        Self::new_with_private_values(
            stdin,
            stdout,
            generation,
            runtime,
            permission_policy,
            prompt_completion,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_with_private_values(
        stdin: ChildStdin,
        stdout: ChildStdout,
        generation: String,
        runtime: &'static str,
        permission_policy: PermissionPolicy,
        prompt_completion: PromptCompletion,
        private_values: Vec<String>,
    ) -> io::Result<Self> {
        let (commands, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (frames, frame_receiver) = mpsc::channel(FRAME_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let permissions = std::sync::Arc::new(Mutex::new(std::collections::BTreeMap::new()));
        let prompt_receipt = std::sync::Arc::new(Mutex::new(None));
        let open_prompt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
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
            std::sync::Arc::clone(&permissions),
            std::sync::Arc::clone(&prompt_receipt),
            private_values,
            std::sync::Arc::clone(&open_prompt),
        ));
        Ok(Self {
            commands,
            kilo_input: OnceLock::new(),
            events,
            task: Mutex::new(Some(task)),
            auxiliary_tasks: Mutex::new(Vec::new()),
            prompt_receipt,
            permissions,
            open_prompt,
        })
    }

    /// Whether a prompt this client wrote was open in its loop when read, from
    /// the completed write until it ends, whether or not the provider has
    /// reported it started. A cancel sent afterwards acts on the turn current
    /// when the loop handles it.
    pub(super) fn has_open_prompt(&self) -> bool {
        self.open_prompt.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) fn set_prompt_receipt(
        &self,
        receipt: super::super::runtime_session::native_acp::PromptReceipt,
    ) {
        *self.prompt_receipt.lock().expect("ACP prompt receipt lock") = Some(receipt);
    }

    pub(super) fn resumable(&self) -> bool {
        self.prompt_receipt
            .lock()
            .is_ok_and(|receipt| receipt.as_ref().is_none_or(|receipt| receipt.resumable()))
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

    pub(super) fn permissions(&self) -> Vec<Value> {
        self.permissions.lock().map(|items| items.iter().map(|(key, message)| {
            match message["method"].as_str() {
                Some("cursor/ask_question") => json!({"request_id":key,"kind":"question","title":message["params"]["title"],"questions":message["params"]["questions"]}),
                Some("cursor/create_plan") => json!({"request_id":key,"kind":"plan","title":message["params"]["name"],"overview":message["params"]["overview"],"plan":message["params"]["plan"],"todos":message["params"]["todos"]}),
                _ => json!({"request_id":key,"kind":"permission","title":message.pointer("/params/toolCall/title").and_then(Value::as_str).unwrap_or("Permission requested"),"details":message.pointer("/params/toolCall/rawInput")}),
            }
        }).collect()).unwrap_or_default()
    }

    pub(super) async fn respond_permission(&self, request_id: &str, allow: bool) -> io::Result<()> {
        self.respond_interaction(request_id, json!({"allow":allow}))
            .await
    }

    pub(super) async fn respond_interaction(
        &self,
        request_id: &str,
        reply: Value,
    ) -> io::Result<()> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::Permission {
                request_id: request_id.to_owned(),
                reply,
                response,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        send_request(&self.commands, method, params, timeout).await
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

    pub(super) async fn register_native_input(
        &self,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::RegisterNativeInput {
                delegation_id: delegation_id.to_owned(),
                text: text.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn forget_native_input(&self, delegation_id: &str) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::ForgetNativeInput {
                delegation_id: delegation_id.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())
    }

    pub(super) async fn cancel(&self, session_id: &str) -> io::Result<()> {
        self.cancel_input(session_id, None).await.map(|_| ())
    }

    /// Cancels the prompt this client wrote, from the moment it was written,
    /// whether or not the provider has reported it started. Answers whether
    /// a prompt was there to cancel.
    pub(super) async fn cancel_current(&self, session_id: &str) -> io::Result<bool> {
        self.cancel_input(session_id, None)
            .await
            .map(|turn| turn.is_some())
    }

    /// Cancels the written prompt and waits for the provider to end it, all
    /// within `bound`: a loop blocked writing to a provider that stopped
    /// reading answers nothing, so every step shares the one deadline.
    /// Used by stop only: it also closes this loop to later prompts.
    pub(super) async fn cancel_and_settle(
        &self,
        session_id: &str,
        bound: Duration,
    ) -> CancelOutcome {
        let deadline = tokio::time::Instant::now() + bound;
        let cancel = self.send_cancel(session_id, None, true);
        let turn = match tokio::time::timeout_at(deadline, cancel).await {
            Ok(Ok(None)) => return CancelOutcome::NoTurn,
            Ok(Ok(Some(turn))) => turn,
            Ok(Err(_)) | Err(_) => return CancelOutcome::Unconfirmed,
        };
        loop {
            match tokio::time::timeout_at(deadline, self.turn_end(&turn)).await {
                Ok(Ok(TurnEnd::Stopped)) => return CancelOutcome::Settled,
                Ok(Ok(TurnEnd::Open)) => {}
                Ok(Ok(TurnEnd::Unknown) | Err(_)) | Err(_) => return CancelOutcome::Requested,
            }
            if tokio::time::timeout_at(deadline, tokio::time::sleep(Duration::from_millis(50)))
                .await
                .is_err()
            {
                return CancelOutcome::Requested;
            }
        }
    }

    async fn turn_end(&self, turn_id: &str) -> io::Result<TurnEnd> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::TurnEnd {
                turn_id: turn_id.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())
    }

    pub(super) async fn cancel_input(
        &self,
        session_id: &str,
        delegation_id: Option<&str>,
    ) -> io::Result<Option<String>> {
        self.send_cancel(session_id, delegation_id, false).await
    }

    async fn send_cancel(
        &self,
        session_id: &str,
        delegation_id: Option<&str>,
        close_admission: bool,
    ) -> io::Result<Option<String>> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(AcpCommand::Cancel {
                session_id: session_id.to_owned(),
                delegation_id: delegation_id.map(str::to_owned),
                close_admission,
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

    /// Closes the protocol loop. Answers `false` when the loop did not end in
    /// time and was aborted: whatever the provider was doing then is unknown.
    pub(super) async fn close(&self) -> bool {
        if let Ok(mut tasks) = self.auxiliary_tasks.lock() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
        // A loop blocked writing to the provider never reads this command, so
        // the deadline covers delivering it as well as the loop ending.
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        let _ = tokio::time::timeout_at(deadline, self.commands.send(AcpCommand::Close)).await;
        let task = self.task.lock().ok().and_then(|mut task| task.take());
        if let Some(mut task) = task
            && tokio::time::timeout_at(deadline, &mut task).await.is_err()
        {
            task.abort();
            let _ = task.await;
            return false;
        }
        true
    }
}

async fn send_request(
    commands: &mpsc::Sender<AcpCommand>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> io::Result<Value> {
    let (sender, receiver) = oneshot::channel();
    commands
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

use super::*;
use serde_json::json;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole cancel, from asking the loop to seeing the turn end. Hermes
/// ended a cancelled turn in about 0.3 s in real runs.
const HERMES_STOP_SETTLE: Duration = Duration::from_secs(2);

/// What stopping did to a prompt this client had written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StopInterrupt {
    /// No prompt of this client needed interrupting before it closed.
    None,
    /// The cancelled prompt answered with an explicit stopReason.
    Settled,
    /// The cancel was written, but no stopReason arrived before close: the
    /// prompt was still open or ended by an error, so its outcome is unknown.
    Requested,
    /// The cancel could not be confirmed written; what the turn does after
    /// the client closed is unknown.
    Unconfirmed,
}

impl StopInterrupt {
    pub(crate) fn as_str(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Settled => Some("settled"),
            Self::Requested => Some("interrupt_requested"),
            Self::Unconfirmed => Some("unconfirmed"),
        }
    }
}

impl AnalystSession {
    pub(crate) async fn cancel_pending_input(
        &self,
        generation: &str,
        delegation_id: &str,
    ) -> io::Result<()> {
        self.require_generation(generation)?;
        let delegation_id = required_value(delegation_id, "delegation_id")?;
        match &self.protocol {
            ManagedProtocol::Acp(protocol) if self.structured_only() => protocol
                .cancel_input(&self.thread_id, Some(delegation_id))
                .await
                .map(|_| ()),
            ManagedProtocol::Pi(protocol) => protocol
                .cancel_input(&self.thread_id, Some(delegation_id))
                .await
                .map(|_| ()),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Runtime does not support pending managed input cancellation",
            )),
        }
    }

    pub(crate) fn permissions(&self) -> Vec<Value> {
        match &self.protocol {
            ManagedProtocol::Acp(protocol) if self.structured_only() => protocol.permissions(),
            ManagedProtocol::Pi(protocol) => protocol.permissions(),
            _ => Vec::new(),
        }
    }

    pub(crate) async fn respond_permission(
        &self,
        generation: &str,
        request_id: &str,
        allow: bool,
    ) -> io::Result<()> {
        self.require_generation(generation)?;
        match &self.protocol {
            ManagedProtocol::Acp(protocol) if self.structured_only() => {
                protocol.respond_permission(request_id, allow).await
            }
            ManagedProtocol::Pi(protocol) => protocol.respond_permission(request_id, allow).await,
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This Runtime handles permissions in its native terminal",
            )),
        }
    }
    pub(crate) async fn respond_interaction(
        &self,
        generation: &str,
        request_id: &str,
        reply: Value,
    ) -> io::Result<()> {
        self.require_generation(generation)?;
        match &self.protocol {
            ManagedProtocol::Acp(protocol) if self.structured_only() => {
                protocol.respond_interaction(request_id, reply).await
            }
            ManagedProtocol::Pi(protocol) => protocol.respond_interaction(request_id, reply).await,
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Runtime does not expose managed interactions",
            )),
        }
    }

    pub(crate) async fn register_native_input(
        &self,
        expected_generation: &str,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        let delegation_id = required_value(delegation_id, "delegation_id")?;
        let text = required_value(text, "text")?;
        self.protocol
            .register_native_input(delegation_id, text)
            .await
    }

    pub(crate) async fn forget_native_input(
        &self,
        expected_generation: &str,
        delegation_id: &str,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        let delegation_id = required_value(delegation_id, "delegation_id")?;
        self.protocol.forget_native_input(delegation_id).await
    }

    pub(crate) async fn steer(
        &self,
        expected_generation: &str,
        turn_id: &str,
        text: &str,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        let turn_id = required_value(turn_id, "turn_id")?;
        let text = required_value(text, "text")?;
        match &self.protocol {
            ManagedProtocol::Codex(protocol) => protocol
                .request(
                    "turn/steer",
                    json!({
                        "threadId":self.thread_id,
                        "expectedTurnId":turn_id,
                        "input":[{"type":"text","text":text}],
                    }),
                    REQUEST_TIMEOUT,
                )
                .await
                .map(|_| ()),
            ManagedProtocol::Acp(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this managed Runtime does not support safe in-turn steering",
            )),
            ManagedProtocol::Claude(_) | ManagedProtocol::Pi(_) | ManagedProtocol::OpenCode(_) => {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "this managed Runtime does not support safe in-turn steering",
                ))
            }
        }
    }

    /// Cancels the ACP prompt this session wrote even before the provider
    /// reports it started; answers whether there was one. Other protocols
    /// only cancel a started turn (`interrupt`).
    pub(crate) async fn cancel_written_prompt(
        &self,
        expected_generation: &str,
    ) -> io::Result<bool> {
        self.require_generation(expected_generation)?;
        match self.written_prompt_canceller() {
            Some(protocol) => protocol.cancel_current(&self.thread_id).await,
            None => Ok(false),
        }
    }

    /// Whether `cancel_written_prompt` had a prompt to cancel when read.
    pub(crate) fn has_cancellable_prompt(&self) -> bool {
        self.written_prompt_canceller()
            .is_some_and(|protocol| protocol.has_open_prompt())
    }

    fn written_prompt_canceller(&self) -> Option<&AcpClient> {
        match &self.protocol {
            ManagedProtocol::Acp(protocol) if self.structured_only() => Some(protocol),
            _ => None,
        }
    }

    pub(crate) async fn interrupt(
        &self,
        expected_generation: &str,
        turn_id: &str,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        let turn_id = required_value(turn_id, "turn_id")?;
        match &self.protocol {
            ManagedProtocol::Codex(protocol) => protocol
                .request(
                    "turn/interrupt",
                    json!({"threadId":self.thread_id,"turnId":turn_id}),
                    REQUEST_TIMEOUT,
                )
                .await
                .map(|_| ()),
            ManagedProtocol::Acp(protocol) => protocol.cancel(&self.thread_id).await,
            ManagedProtocol::OpenCode(protocol) => protocol.cancel(turn_id).await,
            ManagedProtocol::Claude(protocol) => protocol.cancel(turn_id).await,
            ManagedProtocol::Pi(protocol) => protocol.cancel(&self.thread_id, turn_id).await,
        }
    }

    pub(crate) async fn respond_mcp_elicitation(
        &self,
        expected_generation: &str,
        request: &AnalystEvent,
        action: ElicitationAction,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        if !matches!(&self.protocol, ManagedProtocol::Codex(_)) {
            // Non-Codex adapters resolve permissions inside their managed provider path;
            // they never expose a generic request id that can be answered here.
            return Ok(());
        }
        if request.generation != self.generation
            || request.message.get("method").and_then(Value::as_str)
                != Some("mcpServer/elicitation/request")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "event is not an MCP elicitation for this Voice Analyst generation",
            ));
        }
        let id = request
            .message
            .get("id")
            .filter(|id| id.is_number() || id.is_string())
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "elicitation has no id"))?;
        let content = (action == ElicitationAction::Accept).then(|| json!({}));
        self.protocol
            .respond(id, json!({"action":action.as_str(),"content":content}))
            .await
    }

    /// Ask the provider to stop without waiting for confirmation or cleaning
    /// owned resources; see `ManagedProtocol::kill_request`.
    pub(crate) async fn kill_request(&self) -> io::Result<()> {
        self.protocol.kill_request().await
    }

    pub(crate) async fn stop(&self, expected_generation: &str) -> io::Result<()> {
        self.stop_and_report(expected_generation).await.map(|_| ())
    }

    /// Stop, and report what stopping did to a turn this session started.
    pub(crate) async fn stop_and_report(
        &self,
        expected_generation: &str,
    ) -> io::Result<StopInterrupt> {
        let interrupt = self.close_for_stop(expected_generation).await?;
        lifecycle_timing::run_sync("runtime.process_cleanup", || self.cleanup_owned_resources())?;
        Ok(interrupt)
    }

    /// Voice cleanup may wait on OS processes. Keep it off the Web runtime's
    /// worker threads so a stalled child cannot prevent the deadline firing.
    pub(crate) async fn stop_for_voice(
        self: &Arc<Self>,
        expected_generation: &str,
    ) -> io::Result<()> {
        self.close_for_stop(expected_generation).await?;
        let session = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            lifecycle_timing::run_sync("runtime.process_cleanup", || {
                session.cleanup_owned_resources()
            })
        })
        .await
        .map_err(|error| {
            io::Error::other(format!("Voice process cleanup worker failed: {error}"))
        })?
    }

    async fn close_for_stop(&self, expected_generation: &str) -> io::Result<StopInterrupt> {
        self.require_generation(expected_generation)?;
        let interrupt = self.interrupt_gateway_turn().await;
        lifecycle_timing::run("runtime.protocol_close", self.protocol.close()).await?;
        Ok(interrupt)
    }

    /// Hermes runs each turn in its shared gateway, which finishes a turn
    /// after its last client closes, tools included. Cancel the prompt this
    /// client wrote and wait briefly for Hermes to end it; stop continues
    /// either way. Other providers end the turn with their own process.
    async fn interrupt_gateway_turn(&self) -> StopInterrupt {
        let ManagedProtocol::Acp(protocol) = &self.protocol else {
            return StopInterrupt::None;
        };
        if self.runtime != ActorRuntime::Hermes {
            return StopInterrupt::None;
        }
        match protocol
            .cancel_and_settle(&self.thread_id, HERMES_STOP_SETTLE)
            .await
        {
            acp::CancelOutcome::NoTurn => StopInterrupt::None,
            acp::CancelOutcome::Settled => StopInterrupt::Settled,
            acp::CancelOutcome::Requested => {
                tracing::warn!(
                    "Hermes did not confirm the cancelled turn ended before stop closed its client"
                );
                StopInterrupt::Requested
            }
            acp::CancelOutcome::Unconfirmed => {
                tracing::warn!(
                    "Hermes could not be confirmed to receive the cancel before stop closed its client"
                );
                StopInterrupt::Unconfirmed
            }
        }
    }

    /// Signal only processes this session owns. Remote jobs have no such local
    /// handle and still require their own confirmed protocol shutdown.
    pub(crate) fn terminate_local_processes(&self) -> io::Result<bool> {
        let mut attempted = false;
        let mut first_error = None;
        for process in self.process.iter().chain(self.auxiliary_processes.iter()) {
            attempted = true;
            if let Err(error) = process.terminate_now()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(attempted),
        }
    }

    fn cleanup_owned_resources(&self) -> io::Result<()> {
        if let Some(process) = &self.process {
            process.stop()?;
        }
        for process in &self.auxiliary_processes {
            process.stop()?;
        }
        for path in &self.cleanup_paths {
            let removed = if path.is_dir() {
                // Owned launch directories follow their individual files in
                // this list. Unexpected contents remain an explicit failure.
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            };
            match removed {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(crate) fn require_generation(&self, expected: &str) -> io::Result<()> {
        if expected == self.generation {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stale Voice Analyst generation",
            ))
        }
    }
}

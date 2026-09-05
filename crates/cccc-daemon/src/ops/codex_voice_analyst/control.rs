use super::*;
use serde_json::json;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl AnalystSession {
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
            ManagedProtocol::Claude(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this managed Runtime does not support safe in-turn steering",
            )),
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
            ManagedProtocol::Claude(protocol) => protocol.cancel(turn_id).await,
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

    pub(crate) async fn stop(&self, expected_generation: &str) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        lifecycle_timing::run("runtime.protocol_close", self.protocol.close()).await?;
        lifecycle_timing::run_sync("runtime.process_cleanup", || self.cleanup_owned_resources())
    }

    fn cleanup_owned_resources(&self) -> io::Result<()> {
        if let Some(process) = &self.process {
            process.stop()?;
        }
        for process in &self.auxiliary_processes {
            process.stop()?;
        }
        for path in &self.cleanup_paths {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(super) fn require_generation(&self, expected: &str) -> io::Result<()> {
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

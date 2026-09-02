use super::*;
use serde_json::json;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl AnalystSession {
    #[cfg(test)]
    pub(crate) async fn reconnect(self, expected_generation: &str) -> io::Result<Self> {
        self.require_generation(expected_generation)?;
        let Self {
            binding,
            endpoint,
            thread_id,
            remote_tui_prefix,
            environment,
            protocol,
            process,
            delegations,
            ..
        } = self;
        protocol.close().await;
        drop(protocol);
        Self::connect(ConnectConfig {
            binding,
            generation: uuid::Uuid::new_v4().simple().to_string(),
            endpoint,
            remote_tui_prefix,
            environment,
            resume_thread_id: Some(thread_id),
            process,
            delegations: delegations.into_inner(),
            purpose: SessionPurpose::VoiceAnalyst,
        })
        .await
    }

    pub(crate) async fn start_turn(
        &self,
        expected_generation: &str,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<TurnReceipt> {
        self.require_generation(expected_generation)?;
        let delegation_id = required_value(delegation_id, "delegation_id")?;
        let text = required_value(text, "text")?;
        let mut delegations = self.delegations.lock().await;
        if let Some(state) = delegations.get(delegation_id) {
            return match state {
                DelegationState::Started(receipt) => Ok(receipt.clone()),
                DelegationState::Unresolved(reason) => Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "Voice Analyst delegation outcome is unresolved and will not be replayed: {reason}"
                    ),
                )),
            };
        }
        let result = match self
            .protocol
            .request(
                "turn/start",
                json!({
                    "threadId":self.thread_id,
                    "input":[{"type":"text","text":text}],
                    "clientUserMessageId":format!("cccc-voice:{}:{delegation_id}", self.generation),
                    "responsesapiClientMetadata":{
                        "cccc_voice_generation":self.generation,
                        "cccc_turn_correlation_id":delegation_id,
                    }
                }),
                REQUEST_TIMEOUT,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let kind = error.kind();
                let message = error.to_string();
                delegations.insert(
                    delegation_id.to_owned(),
                    DelegationState::Unresolved(message.clone()),
                );
                return Err(io::Error::new(kind, message));
            }
        };
        let turn_id = result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let Some(turn_id) = turn_id else {
            let message = "Codex app-server accepted a delegation without returning a turn id";
            delegations.insert(
                delegation_id.to_owned(),
                DelegationState::Unresolved(message.into()),
            );
            return Err(io::Error::other(message));
        };
        let receipt = TurnReceipt {
            delegation_id: delegation_id.to_owned(),
            thread_id: self.thread_id.clone(),
            turn_id,
        };
        self.mark_thread_materialized();
        delegations.insert(
            delegation_id.to_owned(),
            DelegationState::Started(receipt.clone()),
        );
        Ok(receipt)
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
        self.protocol
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
            .map(|_| ())
    }

    pub(crate) async fn interrupt(
        &self,
        expected_generation: &str,
        turn_id: &str,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
        let turn_id = required_value(turn_id, "turn_id")?;
        self.protocol
            .request(
                "turn/interrupt",
                json!({"threadId":self.thread_id,"turnId":turn_id}),
                REQUEST_TIMEOUT,
            )
            .await
            .map(|_| ())
    }

    pub(crate) async fn respond_mcp_elicitation(
        &self,
        expected_generation: &str,
        request: &AnalystEvent,
        action: ElicitationAction,
    ) -> io::Result<()> {
        self.require_generation(expected_generation)?;
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
        self.protocol.close().await;
        if let Some(process) = &self.process {
            process.stop()?;
        }
        Ok(())
    }

    fn require_generation(&self, expected: &str) -> io::Result<()> {
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

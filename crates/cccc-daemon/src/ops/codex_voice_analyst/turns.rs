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
            auxiliary_processes,
            native_tui_command,
            cleanup_paths,
            delegations,
            ..
        } = self;
        protocol.close().await;
        drop(protocol);
        if !auxiliary_processes.is_empty()
            || native_tui_command.is_some()
            || !cleanup_paths.is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "ACP test reconnect must restart its managed topology",
            ));
        }
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
        let turn_result = match &self.protocol {
            ManagedProtocol::Codex(protocol) => protocol
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
                .and_then(|result| {
                    result
                        .get("turn")
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            io::Error::other(
                                "Codex app-server accepted a delegation without returning a turn id",
                            )
                        })
                }),
            ManagedProtocol::Acp(protocol) => protocol
                .start_prompt(&self.thread_id, delegation_id, text)
                .await,
        };
        let turn_id = match turn_result {
            Ok(turn_id) => turn_id,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(error);
            }
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
}

use super::delegation::validate_delegation;
use super::projection::{
    CallProjection, delegation_context_commands, session_context_commands, validate_final_result,
};
use super::*;
use anyhow::{Context, Result, bail};
use serde_json::Value;

impl CodexVoiceCall {
    #[cfg(test)]
    pub async fn launch(home: &HomeLayout, analyst: LaunchConfig) -> Result<Self> {
        let analyst = Arc::new(CodexVoiceAnalyst::launch(home, analyst).await?);
        Self::start(home, analyst).await
    }

    pub async fn start(home: &HomeLayout, analyst: Arc<CodexVoiceAnalyst>) -> Result<Self> {
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let lease = CallLease::acquire(home, "", "", &format!("codex-voice:{generation}"))?;
        Ok(Self {
            generation,
            analyst,
            lease,
            state: tokio::sync::Mutex::new(CallState::default()),
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    #[cfg(test)]
    pub fn analyst_thread_id(&self) -> &str {
        self.analyst.thread_id()
    }

    #[cfg(test)]
    pub fn analyst_tui_command(&self) -> Vec<String> {
        self.analyst.tui_command()
    }

    #[cfg(test)]
    pub fn analyst_tui_ready(&self) -> bool {
        self.analyst.tui_ready()
    }

    #[cfg(test)]
    pub fn subscribe_analyst(&self) -> broadcast::Receiver<AnalystEvent> {
        self.analyst.subscribe()
    }

    pub fn analyst(&self) -> Arc<CodexVoiceAnalyst> {
        Arc::clone(&self.analyst)
    }

    pub fn heartbeat(&self, expected_generation: &str) -> Result<()> {
        self.require_generation(expected_generation)?;
        self.lease.heartbeat()
    }

    #[cfg(test)]
    pub async fn begin_provider_event(
        &self,
        expected_generation: &str,
        event: &Value,
    ) -> Result<Option<TurnReceipt>> {
        self.require_generation(expected_generation)?;
        let Some(delegation) = parse_provider_delegation(event)? else {
            return Ok(None);
        };
        self.begin_delegation(expected_generation, &delegation)
            .await
            .map(Some)
    }

    pub async fn route_provider_event(
        &self,
        expected_generation: &str,
        event: &Value,
    ) -> Result<Option<VoiceDelegationAdmission>> {
        self.require_generation(expected_generation)?;
        let Some(delegation) = parse_provider_delegation(event)? else {
            return Ok(None);
        };
        validate_delegation(&delegation)?;
        let admission = self
            .analyst
            .lifecycle
            .admit_voice(&delegation.id, &delegation.text)
            .await?;
        if let VoiceDelegationAdmission::Turn(receipt) = &admission {
            self.follow_analyst_turn(receipt).await;
        }
        Ok(Some(admission))
    }

    pub async fn reject_native_delegation(
        &self,
        expected_generation: &str,
        delegation_id: &str,
    ) -> Result<bool> {
        self.require_generation(expected_generation)?;
        self.analyst
            .lifecycle
            .reject_native_voice(delegation_id)
            .await
    }

    #[cfg(test)]
    pub(super) async fn begin_delegation(
        &self,
        expected_generation: &str,
        delegation: &ProviderDelegation,
    ) -> Result<TurnReceipt> {
        self.require_generation(expected_generation)?;
        validate_delegation(delegation)?;
        let admission = self
            .analyst
            .lifecycle
            .admit_voice(&delegation.id, &delegation.text)
            .await?;
        let turn = match admission {
            VoiceDelegationAdmission::Turn(turn) => turn,
            VoiceDelegationAdmission::NativeInput { delegation_id, .. } => {
                let _ = self
                    .analyst
                    .lifecycle
                    .reject_native_voice(&delegation_id)
                    .await?;
                bail!("test caller cannot deliver a native Runtime Voice input")
            }
            VoiceDelegationAdmission::NativeInputPending => {
                bail!("test caller cannot replay a pending native Runtime Voice input")
            }
        };
        self.follow_analyst_turn(&turn).await;
        Ok(turn)
    }

    #[cfg(test)]
    pub async fn steer(
        &self,
        expected_generation: &str,
        delegation_id: &str,
        text: &str,
    ) -> Result<()> {
        self.require_generation(expected_generation)?;
        self.analyst
            .lifecycle
            .steer_voice(delegation_id, text)
            .await
    }

    #[cfg(test)]
    pub async fn cancel(&self, expected_generation: &str, delegation_id: &str) -> Result<()> {
        self.require_generation(expected_generation)?;
        self.analyst.lifecycle.cancel_voice(delegation_id).await?;
        Ok(())
    }

    pub async fn cancel_current(&self, expected_generation: &str) -> Result<bool> {
        self.require_generation(expected_generation)?;
        self.analyst.lifecycle.cancel_current().await
    }

    pub async fn follow_analyst_turn(&self, receipt: &TurnReceipt) {
        let mut state = self.state.lock().await;
        state
            .projections
            .entry(receipt.turn_id.clone())
            .and_modify(|projection| projection.delegation_id = receipt.delegation_id.clone())
            .or_insert_with(|| CallProjection {
                delegation_id: receipt.delegation_id.clone(),
                ..CallProjection::default()
            });
    }

    pub async fn project_analyst_delta(
        &self,
        expected_generation: &str,
        analyst_turn_id: &str,
        delta: &str,
    ) -> Result<Vec<Value>> {
        self.require_generation(expected_generation)?;
        if delta.is_empty() {
            return Ok(Vec::new());
        }
        let mut state = self.state.lock().await;
        let Some(projection) = state.projections.get_mut(analyst_turn_id) else {
            return Ok(Vec::new());
        };
        if projection.projected {
            return Ok(Vec::new());
        }
        let chunks = projection.progress.push(delta)?;
        Ok(chunks
            .into_iter()
            .flat_map(|chunk| session_context_commands(&chunk))
            .collect())
    }

    pub async fn take_final_projection(
        &self,
        expected_generation: &str,
        delegation_id: &str,
        analyst_turn_id: &str,
        result: &str,
    ) -> Result<Option<FinalProjection>> {
        self.require_generation(expected_generation)?;
        let result = validate_final_result(result)?;
        let mut call_state = self.state.lock().await;
        let Some(projection) = call_state.projections.get_mut(analyst_turn_id) else {
            return Ok(None);
        };
        if projection.projected {
            return Ok(None);
        }
        if projection.delegation_id.is_empty() && !delegation_id.trim().is_empty() {
            projection.delegation_id = delegation_id.trim().to_owned();
        }
        let target = projection.delegation_id.clone();
        let had_progress = projection.progress.streamed();
        let remaining = projection.progress.finish(result);
        let commands = if had_progress {
            remaining
                .into_iter()
                .flat_map(|chunk| session_context_commands(&chunk))
                .collect()
        } else {
            remaining
                .into_iter()
                .flat_map(|chunk| delegation_context_commands(&target, &chunk))
                .collect()
        };
        projection.projected = true;
        Ok(Some(FinalProjection {
            delegation_id: target,
            commands,
        }))
    }

    pub async fn settle_without_projection(
        &self,
        expected_generation: &str,
        analyst_turn_id: &str,
    ) -> Result<bool> {
        self.require_generation(expected_generation)?;
        let mut state = self.state.lock().await;
        let Some(projection) = state.projections.get_mut(analyst_turn_id) else {
            return Ok(false);
        };
        if projection.projected {
            return Ok(false);
        }
        projection.projected = true;
        projection.progress = Default::default();
        Ok(true)
    }

    pub async fn stop(&self, expected_generation: &str) -> Result<()> {
        self.require_generation(expected_generation)?;
        self.lease
            .release()
            .context("release Codex Voice microphone lease")
    }

    fn require_generation(&self, expected: &str) -> Result<()> {
        if expected == self.generation {
            Ok(())
        } else {
            bail!("stale Codex Voice call generation")
        }
    }
}

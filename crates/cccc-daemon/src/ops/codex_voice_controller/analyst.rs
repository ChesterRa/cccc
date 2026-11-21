use super::*;
use crate::ops::codex_voice_lifecycle::{AnalystCompletionSource, GroupWaitDeadline};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

impl CodexVoiceAnalyst {
    pub async fn launch(home: &HomeLayout, config: LaunchConfig) -> Result<Self> {
        let session = AnalystSession::launch(home, config)
            .await
            .context("launch global Voice Analyst")?;
        Ok(Self::from_session(session))
    }

    pub(super) fn from_session(session: AnalystSession) -> Self {
        let session = Arc::new(session);
        let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
        Self { session, lifecycle }
    }

    pub fn generation(&self) -> &str {
        self.session.generation()
    }

    pub fn resumable(&self) -> bool {
        self.session.resumable()
    }

    pub fn thread_id(&self) -> &str {
        self.session.thread_id()
    }

    pub fn tui_command(&self) -> Vec<String> {
        self.session.tui_command()
    }

    pub fn tui_environment(&self) -> BTreeMap<String, String> {
        self.session.tui_environment()
    }

    pub fn tui_ready(&self) -> bool {
        self.session.tui_ready()
    }

    pub fn structured_only(&self) -> bool {
        self.session.structured_only()
    }
    pub fn queued_inputs(&self) -> usize {
        self.lifecycle.queued_inputs()
    }

    /// In-flight lifecycle IDs, or None if their owner is currently busy.
    pub fn try_in_flight(&self) -> Option<(Vec<String>, Vec<String>)> {
        self.lifecycle.try_in_flight()
    }
    pub fn permissions(&self) -> Vec<serde_json::Value> {
        self.session.permissions()
    }
    pub async fn respond_permission(
        &self,
        generation: &str,
        request_id: &str,
        allow: bool,
    ) -> Result<()> {
        self.session
            .respond_permission(generation, request_id, allow)
            .await
            .map_err(Into::into)
    }
    pub async fn respond_interaction(
        &self,
        generation: &str,
        request_id: &str,
        reply: serde_json::Value,
    ) -> Result<()> {
        self.session
            .respond_interaction(generation, request_id, reply)
            .await
            .map_err(Into::into)
    }
    pub async fn submit_input(&self, id: &str, text: &str) -> Result<VoiceDelegationAdmission> {
        self.lifecycle.admit_terminal(id, text).await
    }

    pub async fn submit_group_input(
        &self,
        id: &str,
        text: &str,
    ) -> Result<VoiceDelegationAdmission> {
        self.lifecycle.admit_group(id, text).await
    }

    pub async fn submit_chet_text(&self, id: &str, text: &str) -> Result<VoiceDelegationAdmission> {
        self.lifecycle.admit_chet_text(id, text).await
    }

    /// An uncertain HTTP admission closes this bound OpenCode connection before
    /// returning to the caller. Local queue or validation errors leave it live.
    pub fn opencode_connection_lost(&self) -> bool {
        self.session.runtime() == cccc_contracts::ActorRuntime::Opencode
            && !self.session.process_running()
    }

    pub async fn observe_group_wait(&self, deadline: &GroupWaitDeadline) {
        self.lifecycle.observe_group_wait(deadline).await;
    }

    pub async fn submit_group_input_until(
        &self,
        id: &str,
        text: &str,
        deadline: &GroupWaitDeadline,
    ) -> Result<VoiceDelegationAdmission> {
        self.lifecycle.admit_group_until(id, text, deadline).await
    }

    pub async fn group_wait_expired(&self, id: &str) -> bool {
        self.lifecycle.group_wait_expired(id).await
    }

    /// Call only after the daemon confirms the Group request's durable expiry.
    pub async fn confirm_group_wait_expiry(&self, id: &str) {
        self.lifecycle
            .settle_group_wait(id, AnalystCompletionSource::HostGroupWaitExpired)
            .await;
    }

    /// A different durable outcome already exists. Release the reservation without
    /// sending the Group request or replacing its existing history.
    pub async fn release_group_wait_unconfirmed(&self, id: &str) {
        self.lifecycle
            .settle_group_wait(id, AnalystCompletionSource::HostUnconfirmed)
            .await;
    }

    /// A retired console row must not turn a previously accepted input into new work.
    pub async fn input_was_admitted(&self, id: &str) -> bool {
        self.lifecycle.input_was_admitted(id).await
    }

    #[cfg(test)]
    pub fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.session.subscribe()
    }

    pub fn subscribe_lifecycle(&self) -> broadcast::Receiver<AnalystLifecycleEvent> {
        self.lifecycle.subscribe()
    }

    pub async fn begin_actor_result(
        &self,
        correlation_id: &str,
        text: &str,
        speakable: bool,
    ) -> Result<VoiceDelegationAdmission> {
        self.lifecycle
            .begin_actor_result(correlation_id, text, speakable)
            .await
    }

    pub async fn reject_native_input(&self, correlation_id: &str) -> Result<bool> {
        self.lifecycle.reject_native_voice(correlation_id).await
    }

    pub async fn is_busy(&self) -> bool {
        self.lifecycle.is_busy().await
    }

    pub async fn terminal_input_allowed(&self) -> bool {
        self.lifecycle.terminal_input_allowed().await
    }

    pub async fn cancel_current(&self) -> Result<bool> {
        self.lifecycle.cancel_current().await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.session
            .stop_for_voice(self.session.generation())
            .await
            .context("stop Voice Analyst")
    }

    /// Send an emergency signal only to this Analyst's owned local processes.
    /// A successful signal does not itself confirm shutdown.
    pub fn terminate_local_processes(&self) -> Result<bool> {
        self.session
            .terminate_local_processes()
            .context("terminate owned Voice Analyst processes")
    }
}

use super::*;
use anyhow::{Context, Result};

impl CodexVoiceAnalyst {
    pub async fn launch(home: &HomeLayout, config: LaunchConfig) -> Result<Self> {
        let session = AnalystSession::launch(home, config)
            .await
            .context("launch repository-bound Voice Analyst")?;
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

    pub fn thread_id(&self) -> &str {
        self.session.thread_id()
    }

    pub fn tui_command(&self) -> Vec<String> {
        self.session.tui_command()
    }

    pub fn tui_ready(&self) -> bool {
        self.session.tui_ready()
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
    ) -> Result<TurnReceipt> {
        self.lifecycle
            .begin_actor_result(correlation_id, text, speakable)
            .await
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
            .stop(self.session.generation())
            .await
            .context("stop Voice Analyst")
    }
}

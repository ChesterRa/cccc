use super::*;
use anyhow::{Result, anyhow};

impl ActiveSession {
    pub(crate) fn call(&self) -> &Arc<CodexVoiceCall> {
        &self.call
    }

    pub(crate) fn analyst(&self) -> &Arc<AnalystRuntime> {
        &self.analyst
    }

    pub(crate) fn info(&self) -> SessionInfo {
        SessionInfo {
            group_id: self.group_id.clone(),
            group_title: self.group_title.clone(),
            generation: self.call.generation().to_owned(),
            analyst_generation: self.analyst.analyst.generation().to_owned(),
            voice: self.voice.clone(),
            connected: self.connection_state.load(Ordering::Acquire) == CONNECTION_ATTACHED,
        }
    }

    pub(super) fn attach(self: &Arc<Self>) -> Result<SessionAttachment> {
        self.connection_state
            .compare_exchange(
                CONNECTION_UNATTACHED,
                CONNECTION_ATTACHED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| anyhow!("Codex Voice call already has a browser connection"))?;
        Ok(SessionAttachment {
            session: Arc::clone(self),
        })
    }
}

impl SessionAttachment {
    pub(crate) fn session(&self) -> &Arc<ActiveSession> {
        &self.session
    }
}

impl Drop for SessionAttachment {
    fn drop(&mut self) {
        let _ = self.session.connection_state.compare_exchange(
            CONNECTION_ATTACHED,
            CONNECTION_UNATTACHED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

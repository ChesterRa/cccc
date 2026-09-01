use super::*;
use anyhow::{Context, Result, anyhow, bail};

impl CodexVoiceSessions {
    pub(crate) async fn heartbeat_if_unattached(
        &self,
        group_id: &str,
        generation: &str,
    ) -> Result<bool> {
        let session = {
            let state = self.state.lock().await;
            let Some(session) = state.active.as_ref() else {
                return Ok(false);
            };
            if session.group_id != group_id || session.call.generation() != generation {
                return Ok(false);
            }
            if session.connection_state.load(Ordering::Acquire) != CONNECTION_UNATTACHED {
                return Ok(false);
            }
            Arc::clone(session)
        };
        session
            .call
            .heartbeat(generation)
            .context("renew unattached Codex Voice recording lease")?;
        Ok(true)
    }

    pub(crate) async fn stop(&self, group_id: &str, generation: &str) -> Result<bool> {
        let session = {
            let mut state = self.state.lock().await;
            let Some(session) = state.active.as_ref() else {
                return Ok(false);
            };
            if session.group_id != group_id || session.call.generation() != generation {
                return Ok(false);
            }
            session
                .connection_state
                .store(CONNECTION_CLOSING, Ordering::Release);
            state
                .active
                .take()
                .expect("checked active Codex Voice call")
        };
        session.analyst.set_call_generation(None);
        session
            .call
            .stop(generation)
            .await
            .context("stop Codex Voice audio call")?;
        Ok(true)
    }

    pub(crate) async fn stop_if_unattached(
        &self,
        group_id: &str,
        generation: &str,
    ) -> Result<bool> {
        let session = {
            let mut state = self.state.lock().await;
            let Some(session) = state.active.as_ref() else {
                return Ok(false);
            };
            if session.group_id != group_id || session.call.generation() != generation {
                return Ok(false);
            }
            if session
                .connection_state
                .compare_exchange(
                    CONNECTION_UNATTACHED,
                    CONNECTION_CLOSING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return Ok(false);
            }
            state
                .active
                .take()
                .expect("checked active Codex Voice call")
        };
        session.analyst.set_call_generation(None);
        session
            .call
            .stop(generation)
            .await
            .context("stop unattached Codex Voice audio call")?;
        Ok(true)
    }

    pub(crate) async fn reset_analyst(
        &self,
        home: &HomeLayout,
        group_id: &str,
        analyst_generation: &str,
    ) -> Result<AnalystInfo> {
        let mut state = self.state.lock().await;
        if state.active.is_some() {
            bail!("Stop the active Codex Voice call before starting a new Analyst session");
        }
        let previous = state
            .analyst
            .as_ref()
            .filter(|analyst| {
                analyst.group_id == group_id && analyst.analyst.generation() == analyst_generation
            })
            .cloned()
            .ok_or_else(|| anyhow!("Voice Analyst is no longer active"))?;
        if previous.analyst.is_busy().await {
            bail!(
                "Wait for or cancel the current Voice Analyst investigation before starting a new Analyst session"
            );
        }
        let replacement = Arc::new(
            persistence::launch_fresh_analyst(
                home,
                &previous.group_id,
                &previous.group_title,
                &previous.root,
                String::new(),
            )
            .await
            .context("start a new Voice Analyst session")?,
        );
        replacement.start_monitor(home.clone(), self.ledger_events.clone());
        persistence::persist_analyst(home, &replacement, false)?;
        state.analyst = Some(Arc::clone(&replacement));
        previous.stop_terminal();
        previous.analyst.shutdown().await.ok();
        Ok(replacement.info())
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        let (active, analyst) = {
            let mut state = self.state.lock().await;
            (state.active.take(), state.analyst.take())
        };
        let mut first_error = None;
        if let Some(session) = active {
            session
                .connection_state
                .store(CONNECTION_CLOSING, Ordering::Release);
            let generation = session.call.generation().to_owned();
            session.analyst.set_call_generation(None);
            if let Err(error) = session.call.stop(&generation).await {
                first_error = Some(error.context("stop Codex Voice audio call during shutdown"));
            }
        }
        if let Some(analyst) = analyst {
            analyst.stop_terminal();
            if let Err(error) = analyst.analyst.shutdown().await
                && first_error.is_none()
            {
                first_error = Some(error.context("stop Voice Analyst during shutdown"));
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }
}

impl Default for CodexVoiceSessions {
    fn default() -> Self {
        Self {
            state: Mutex::new(ManagedState::default()),
            ledger_events: None,
        }
    }
}

use super::*;
use anyhow::{Context, Result, anyhow, bail};

impl CodexVoiceSessions {
    pub(crate) async fn heartbeat_if_unattached(&self, generation: &str) -> Result<bool> {
        let session = {
            let state = self.state.lock().await;
            let Some(session) = state.active.as_ref() else {
                return Ok(false);
            };
            if session.call.generation() != generation {
                return Ok(false);
            }
            if session.connection_state.load(Ordering::Acquire) != CONNECTION_UNATTACHED {
                return Ok(false);
            }
            Arc::clone(session)
        };
        session
            .call
            .heartbeat_for_web(generation)
            .await
            .context("renew unattached Codex Voice recording lease")?;
        Ok(true)
    }

    pub(crate) async fn stop(&self, generation: &str) -> Result<bool> {
        self.stop_call(generation, false).await
    }

    pub(crate) async fn stop_if_unattached(&self, generation: &str) -> Result<bool> {
        self.stop_call(generation, true).await
    }

    async fn stop_call(&self, generation: &str, only_unattached: bool) -> Result<bool> {
        let state = Arc::clone(&self.state);
        let phase = Arc::clone(&self.restart_phase);
        let cleanup = Arc::clone(&self.cleanup_gate);
        let generation = generation.to_owned();
        // Once a stop is admitted, keep its exact owner and cleanup gate even
        // if the HTTP request or socket which initiated it is cancelled.
        tokio::spawn(async move {
            let _cleanup = cleanup.lock_owned().await;
            let session = {
                let state = state.lock().await;
                let Some(session) = state.active.as_ref() else {
                    return Ok(false);
                };
                if session.call.generation() != generation {
                    return Ok(false);
                }
                if only_unattached {
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
                } else {
                    session
                        .connection_state
                        .store(CONNECTION_CLOSING, Ordering::Release);
                }
                Arc::clone(session)
            };
            let mut pending = StopRecoveryOnDrop { phase, armed: true };
            if let Err(error) = session.call.stop_for_web(&generation).await {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice cleanup: recording lease release failed during Stop"
                ));
                return Err(error.context("stop Codex Voice audio call"));
            }
            let mut current = state.lock().await;
            if current
                .active
                .as_ref()
                .is_some_and(|value| Arc::ptr_eq(value, &session))
            {
                current.active = None;
            }
            drop(current);
            if let Some(analyst) = session.analyst.as_ref() {
                analyst.retire_manual_call(&generation);
            }
            pending.armed = false;
            Ok(true)
        })
        .await
        .context("Codex Voice stop worker failed")?
    }

    pub(crate) async fn reset_analyst(
        self: &Arc<Self>,
        home: &HomeLayout,
        analyst_generation: &str,
    ) -> Result<AnalystInfo> {
        if self.restart_phase.load(Ordering::Acquire) == restart::RECOVERY_PENDING {
            return Err(restart::VoiceGateError::ResetRecoveryPending.into());
        }
        self.ensure_open()?;
        let state = self.state.lock().await;
        if self.restart_phase.load(Ordering::Acquire) == restart::RECOVERY_PENDING {
            return Err(restart::VoiceGateError::ResetRecoveryPending.into());
        }
        self.ensure_open()?;
        if state.active.is_some() {
            bail!("Stop the active Codex Voice call before starting a new Analyst session");
        }
        let previous = state
            .analyst
            .as_ref()
            .filter(|analyst| analyst.analyst.generation() == analyst_generation)
            .cloned()
            .ok_or_else(|| anyhow!("Voice Analyst is no longer active"))?;
        if previous.analyst.is_busy().await {
            bail!(
                "Wait for or cancel the current Voice Analyst investigation before starting a new Analyst session"
            );
        }
        // Reserve the owner before releasing the state lock. Once admitted,
        // finish the reset even if the HTTP caller disconnects; its outcome
        // may then be unknown to that caller, but it must not spawn twice.
        self.reserve_cleanup(false)?;
        let sessions = Arc::clone(self);
        let home = home.clone();
        drop(state);
        tokio::spawn(async move {
            sessions
                .cleanup_reserved(restart::PREPARED)
                .await
                .map_err(|error| match error {
                    restart::VoiceGateError::RecoveryPending => {
                        restart::VoiceGateError::ResetRecoveryPending
                    }
                    other => other,
                })?;
            let _recover_on_failure = sessions.prepared_reset_guard();
            let _cleanup = sessions.cleanup_gate.lock().await;
            if sessions.restart_phase.load(Ordering::Acquire) != restart::PREPARED {
                return Err(restart::VoiceGateError::Restarting.into());
            }
            let mut state = sessions.state.lock().await;
            if state.active.is_some() || state.analyst.is_some() {
                return Err(restart::VoiceGateError::ResetRecoveryPending.into());
            }
            let replacement = Arc::new(persistence::launch_fresh_analyst(&home, String::new())
                .await
                .map_err(|_| {
                    cccc_runtime::diagnostic(format_args!(
                        "[cccc] Voice reset could not launch its replacement; recovery is pending"
                    ));
                    restart::VoiceGateError::ResetRecoveryPending
                })?);
            // Retain the new owner before any further fallible operation, so a
            // failed persistence step can be cleaned on the next user retry.
            state.analyst = Some(Arc::clone(&replacement));
            persistence::persist_analyst(&home, &replacement, false).map_err(|_| {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice reset could not persist its replacement; recovery is pending"
                ));
                restart::VoiceGateError::ResetRecoveryPending
            })?;
            replacement.start_monitor(home);
            sessions.cancel_prepared_restart();
            Ok(replacement.info())
        })
        .await
        .context("Voice Analyst reset worker failed")?
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        self.begin_shutdown();
        let _cleanup = self.cleanup_gate.lock().await;
        self.cleanup_owned().await
    }

    pub(super) async fn cleanup_owned(&self) -> Result<()> {
        let (active, analyst) = {
            let state = self.state.lock().await;
            (state.active.clone(), state.analyst.clone())
        };
        let mut first_error = None;
        if let Some(session) = active {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] Voice cleanup: releasing recording lease"
            ));
            session
                .connection_state
                .store(CONNECTION_CLOSING, Ordering::Release);
            let generation = session.call.generation().to_owned();
            if let Err(error) = session.call.stop_for_web(&generation).await {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice cleanup: recording lease release failed"
                ));
                first_error = Some(error.context("stop Codex Voice audio call during shutdown"));
            } else {
                let mut state = self.state.lock().await;
                if state
                    .active
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    state.active = None;
                }
            }
        }
        if let Some(analyst) = analyst {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] Voice cleanup: stopping Analyst terminal"
            ));
            let terminal = analyst.retire_terminal().await;
            if terminal.is_err() {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice cleanup: Analyst terminal stop failed"
                ));
            }
            cccc_runtime::diagnostic(format_args!(
                "[cccc] Voice cleanup: stopping Analyst protocol and processes"
            ));
            let stopped = analyst.analyst.shutdown().await;
            if stopped.is_err() {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice cleanup: Analyst protocol or process stop failed"
                ));
            }
            if terminal.is_ok() && stopped.is_ok() {
                let mut state = self.state.lock().await;
                if state
                    .analyst
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &analyst))
                {
                    state.analyst = None;
                }
            }
            if first_error.is_none() {
                if let Err(error) = terminal {
                    first_error =
                        Some(error.context("stop Voice Analyst terminal during shutdown"));
                } else if let Err(error) = stopped {
                    first_error = Some(error.context("stop Voice Analyst during shutdown"));
                }
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
            state: Arc::new(Mutex::new(ManagedState::default())),
            restart_phase: Arc::new(AtomicU8::new(restart::OPEN)),
            cleanup_gate: Arc::new(Mutex::new(())),
            process_recovery: None,
        }
    }
}

struct StopRecoveryOnDrop {
    phase: Arc<AtomicU8>,
    armed: bool,
}

impl Drop for StopRecoveryOnDrop {
    fn drop(&mut self) {
        if self.armed {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] Voice cleanup: Stop did not finish; recovery is pending"
            ));
            let _ = self.phase.compare_exchange(
                restart::OPEN,
                restart::RECOVERY_PENDING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
}

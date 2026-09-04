use super::*;
use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::time::Duration;

impl CodexVoiceSessions {
    pub(crate) fn new(ledger_events: LedgerEventHub) -> Self {
        Self {
            state: Mutex::new(ManagedState::default()),
            ledger_events: Some(ledger_events),
        }
    }

    pub(crate) async fn start(
        &self,
        home: &HomeLayout,
        client_session_id: &str,
        offer_sdp: &str,
        voice: &str,
    ) -> Result<StartOutcome> {
        let client_session_id = persistence::validate_client_session_id(client_session_id)?;
        let analyst_settings = cccc_core::codex_voice_settings::load(home)?;
        let custom_environment = cccc_core::codex_voice_settings::private_environment(home)?;
        let analyst_runtime =
            cccc_core::codex_voice_settings::resolve(home, &analyst_settings, &custom_environment)?;
        let realtime = RealtimeCallConfig::from_environment_with_voice(voice)?;
        let offer_digest: [u8; 32] = Sha256::digest(offer_sdp.as_bytes()).into();
        // The manager lock intentionally serializes the slow launch. Releasing it would require a
        // second reservation state and could create two provider calls for one microphone lease.
        let mut state = self.state.lock().await;
        if let Some(session) = state.active.as_ref() {
            if session.client_session_id == client_session_id
                && session.offer_digest == offer_digest
                && session.voice == realtime.voice
            {
                return Ok(StartOutcome::Started(StartedSession {
                    session: Arc::clone(session),
                    answer_sdp: session.answer_sdp.clone(),
                    newly_created: false,
                }));
            }
            return Ok(StartOutcome::Busy(session.info()));
        }

        let analyst = state
            .analyst
            .as_ref()
            .is_some_and(|analyst| {
                analyst.reusable_for_call() && analyst.matches_launch(analyst_runtime.fingerprint())
            })
            .then(|| Arc::clone(state.analyst.as_ref().expect("reusable Voice Analyst")));
        let analyst = if let Some(analyst) = analyst {
            analyst
        } else {
            let previous = state.analyst.take();
            if let Some(previous) = previous.as_ref()
                && previous.analyst.is_busy().await
            {
                state.analyst = Some(Arc::clone(previous));
                bail!(
                    "Wait for or cancel the current Voice Analyst investigation before replacing it"
                );
            }
            if let Some(previous) = previous.as_ref() {
                // A disconnected Claude replacement may resume the exact same Agent View job.
                // Relinquish the old owner first; stopping it after launch would kill the job
                // that the replacement just adopted.
                previous.stop_terminal();
                if let Err(error) = previous.analyst.shutdown().await {
                    state.analyst = Some(Arc::clone(previous));
                    return Err(error.context(
                        "stop previous Voice Analyst before resuming its persistent session",
                    ));
                }
            }
            let analyst = Arc::new(
                persistence::launch_analyst(home)
                    .await
                    .context("launch persistent Voice Analyst")?,
            );
            analyst.start_monitor(home.clone(), self.ledger_events.clone());
            if let Err(error) =
                persistence::persist_analyst(home, &analyst, analyst.analyst.tui_ready())
            {
                analyst.stop_terminal();
                return match analyst.analyst.shutdown().await {
                    Ok(()) => Err(error.context("persist replacement Voice Analyst receipt")),
                    Err(cleanup) => Err(anyhow!(
                        "persist replacement Voice Analyst receipt: {error}; replacement cleanup also failed: {cleanup}"
                    )),
                };
            }
            state.analyst = Some(Arc::clone(&analyst));
            analyst
        };

        let call = CodexVoiceCall::start(home, analyst.analyst())
            .await
            .context("start Codex Voice audio call")?;
        let generation = call.generation().to_owned();
        let answer_sdp =
            match create_realtime_answer_with_heartbeat(&call, &realtime, offer_sdp).await {
                Ok(answer) => answer,
                Err(error) => {
                    let _ = call.stop(&generation).await;
                    return Err(error.context("establish Codex Realtime Voice"));
                }
            };
        let session = Arc::new(ActiveSession {
            call: Arc::new(call),
            analyst,
            client_session_id,
            offer_digest,
            answer_sdp: answer_sdp.clone(),
            voice: realtime.voice,
            connection_state: AtomicU8::new(CONNECTION_UNATTACHED),
        });
        session.analyst.set_call_generation(Some(&generation));
        state.active = Some(Arc::clone(&session));
        Ok(StartOutcome::Started(StartedSession {
            session,
            answer_sdp,
            newly_created: true,
        }))
    }

    pub(crate) async fn current(&self) -> VoiceState {
        let (call, analyst) = {
            let state = self.state.lock().await;
            (
                state.active.as_ref().map(|session| session.info()),
                state.analyst.clone(),
            )
        };
        let analyst = if let Some(analyst) = analyst {
            let busy = analyst.analyst.is_busy().await;
            Some(with_authoritative_busy(analyst.info(), busy))
        } else {
            None
        };
        VoiceState { call, analyst }
    }

    pub(crate) async fn attach(&self, generation: &str) -> Result<SessionAttachment> {
        let state = self.state.lock().await;
        let session = state
            .active
            .as_ref()
            .filter(|session| session.call.generation() == generation)
            .ok_or_else(|| anyhow!("Codex Voice call is no longer active"))?;
        session.attach()
    }

    pub(crate) async fn terminal_session(
        &self,
        analyst_generation: &str,
    ) -> Result<Arc<AnalystRuntime>> {
        let analyst = self.require_analyst(analyst_generation).await?;
        if !analyst.analyst.tui_ready() {
            bail!("Voice Analyst terminal becomes available after its first investigation begins");
        }
        Ok(analyst)
    }

    pub(crate) async fn cancel_analyst(&self, analyst_generation: &str) -> Result<bool> {
        self.require_analyst(analyst_generation)
            .await?
            .analyst
            .cancel_current()
            .await
    }

    async fn require_analyst(&self, analyst_generation: &str) -> Result<Arc<AnalystRuntime>> {
        self.state
            .lock()
            .await
            .analyst
            .as_ref()
            .filter(|analyst| analyst.analyst.generation() == analyst_generation)
            .cloned()
            .ok_or_else(|| anyhow!("Voice Analyst is no longer active"))
    }
}

fn with_authoritative_busy(mut info: AnalystInfo, busy: bool) -> AnalystInfo {
    if info.phase != "needs_attention" {
        if busy {
            info.phase = "working".into();
        } else if info.phase == "working" {
            info.phase = "ready".into();
        }
    }
    info
}

async fn create_realtime_answer_with_heartbeat(
    call: &CodexVoiceCall,
    realtime: &RealtimeCallConfig,
    offer_sdp: &str,
) -> Result<String> {
    let generation = call.generation().to_owned();
    let answer = create_realtime_answer(realtime, offer_sdp);
    tokio::pin!(answer);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            result = &mut answer => {
                let answer = result?;
                call.heartbeat(&generation)
                    .context("renew Codex Voice recording lease after provider handshake")?;
                return Ok(answer);
            }
            _ = heartbeat.tick() => {
                call.heartbeat(&generation)
                    .context("renew Codex Voice recording lease during provider handshake")?;
            }
        }
    }
}

#[cfg(test)]
mod current_tests {
    use super::with_authoritative_busy;
    use crate::codex_voice::AnalystInfo;

    fn info(phase: &str) -> AnalystInfo {
        AnalystInfo {
            generation: "analyst-1".into(),
            tui_ready: true,
            phase: phase.into(),
            last_result: String::new(),
            warning: String::new(),
        }
    }

    #[test]
    fn current_state_projects_pending_runtime_work_without_hiding_attention() {
        assert_eq!(
            with_authoritative_busy(info("ready"), true).phase,
            "working"
        );
        assert_eq!(
            with_authoritative_busy(info("working"), false).phase,
            "ready"
        );
        assert_eq!(
            with_authoritative_busy(info("needs_attention"), true).phase,
            "needs_attention"
        );
    }
}

use super::*;
use anyhow::{Context, Result};
use cccc_core::voice_notifications as store;
use cccc_daemon::experimental_codex_voice::VoiceDelegationAdmission;
use std::time::Duration;

impl ActiveSession {
    pub(crate) fn start_notifications(
        self: &Arc<Self>,
        home: HomeLayout,
        events: LedgerEventHub,
        principal: crate::auth::Principal,
    ) {
        let weak = Arc::downgrade(self);
        let mut ledger = events.subscribe_global();
        tokio::spawn(async move {
            let mut poll = tokio::time::interval(Duration::from_secs(1));
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = poll.tick() => {},
                    event = ledger.recv() => {
                        if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
                        // A lagged broadcast is only a wake-up loss. Replay from disk.
                    }
                }
                let Some(session) = weak.upgrade() else {
                    break;
                };
                if !session.info().connected {
                    break;
                }
                if let Err(error) = session.receive_notifications(&home, &principal).await {
                    tracing::warn!(%error, "Voice notification consumption failed; durable references retained");
                    // Notification ingestion can fail while both the Analyst and
                    // audio remain usable. Publish its own state to the call owner.
                    session.notification_paused.send_replace(true);
                    break;
                }
            }
        });
    }

    async fn receive_notifications(
        &self,
        home: &HomeLayout,
        principal: &crate::auth::Principal,
    ) -> Result<()> {
        if !principal.current_voice_admin(home)? {
            return Ok(());
        }
        store::scan(home).context("scan Voice ledger increments")?;
        for item in store::snapshot(home)?.messages {
            if !self.info().connected || !principal.current_voice_admin(home)? {
                break;
            }
            if item.handoff.is_some() {
                continue;
            }
            let generation = self.analyst.analyst.generation();
            let Some(event) = store::reserve(home, &item.source, generation)? else {
                continue;
            };
            let prompt = store::notification_prompt(home, &event, self.verbosity)?;
            let id = item.source.correlation_id();
            let admission = self
                .analyst
                .analyst
                .begin_actor_result(&id, &prompt, false)
                .await
                .context("deliver source data to Voice Analyst")?;
            match admission {
                VoiceDelegationAdmission::NativeInput {
                    delegation_id,
                    text,
                } => {
                    let delivered = self.analyst.submit_native_voice_input(&text).await;
                    if !matches!(delivered, Ok(true)) {
                        let rolled_back = self
                            .analyst
                            .analyst
                            .reject_native_input(&delegation_id)
                            .await?;
                        if rolled_back {
                            anyhow::bail!(
                                "native Runtime did not accept the Voice notification input"
                            );
                        }
                    }
                }
                VoiceDelegationAdmission::Turn(_)
                | VoiceDelegationAdmission::NativeInputPending => {}
            }
            store::accepted(home, &item.source, generation)?;
        }
        Ok(())
    }
}

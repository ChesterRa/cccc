use super::*;
#[cfg(test)]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};

impl AnalystLifecycle {
    pub(crate) async fn begin_voice(&self, delegation_id: &str, text: &str) -> Result<TurnReceipt> {
        self.begin(delegation_id, text, AnalystTurnOrigin::Voice)
            .await
    }

    pub(crate) async fn begin_actor_result(
        &self,
        correlation_id: &str,
        text: &str,
        speakable: bool,
    ) -> Result<TurnReceipt> {
        self.begin(
            correlation_id,
            text,
            AnalystTurnOrigin::ActorResult { speakable },
        )
        .await
    }

    async fn begin(
        &self,
        delegation_id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
    ) -> Result<TurnReceipt> {
        {
            let mut state = self.state.lock().await;
            if let Some(receipt) = state.delegations.get(delegation_id) {
                return Ok(receipt.clone());
            }
            if let Some(active) = state.active.as_mut() {
                if active.cancelling {
                    bail!("Voice Analyst turn is cancelling");
                }
                if active.origin != AnalystTurnOrigin::Voice || origin != AnalystTurnOrigin::Voice {
                    bail!("Voice Analyst is busy with another investigation");
                }
                self.session
                    .steer(self.session.generation(), &active.turn_id, text)
                    .await
                    .context("steer active Voice Analyst delegation")?;
                active.latest_delegation_id = delegation_id.to_owned();
                let receipt = TurnReceipt {
                    delegation_id: delegation_id.to_owned(),
                    thread_id: self.session.thread_id().to_owned(),
                    turn_id: active.turn_id.clone(),
                };
                state
                    .delegations
                    .insert(delegation_id.to_owned(), receipt.clone());
                return Ok(receipt);
            }
            if state.pending.is_some() {
                bail!("Voice Analyst is starting another investigation");
            }
            state.pending = Some(PendingStart {
                delegation_id: delegation_id.to_owned(),
                origin,
            });
            state.settled_pending = None;
        }

        // Codex may publish `turn/started` before answering `turn/start`; the monitor must be able
        // to record that authoritative event while the request is in flight.
        let result = self
            .session
            .start_turn(self.session.generation(), delegation_id, text)
            .await
            .context("start Voice Analyst investigation");
        let mut state = self.state.lock().await;
        if state.pending.as_ref().is_some_and(|pending| {
            pending.delegation_id == delegation_id && pending.origin == origin
        }) {
            state.pending = None;
        }
        let settled_pending = if state
            .settled_pending
            .as_ref()
            .is_some_and(|settled| settled.delegation_id == delegation_id)
        {
            state.settled_pending.take()
        } else {
            None
        };
        match result {
            Ok(receipt) => {
                if let Some(settled) = settled_pending {
                    if settled.turn_id != receipt.turn_id {
                        bail!("Codex completed a different Voice Analyst turn while starting");
                    }
                    state
                        .delegations
                        .insert(delegation_id.to_owned(), receipt.clone());
                    return Ok(receipt);
                }
                let emit_started = if let Some(active) = state.active.as_mut() {
                    if active.turn_id != receipt.turn_id {
                        bail!("Codex reported two concurrent Voice Analyst turns");
                    }
                    if active.latest_delegation_id.is_empty() {
                        active.latest_delegation_id = delegation_id.to_owned();
                    }
                    false
                } else {
                    state.active = Some(active_from(&receipt, origin));
                    true
                };
                state
                    .delegations
                    .insert(delegation_id.to_owned(), receipt.clone());
                if emit_started {
                    let _ = self.events.send(AnalystLifecycleEvent::Started {
                        receipt: receipt.clone(),
                        origin,
                    });
                }
                Ok(receipt)
            }
            Err(error) => {
                if let Some(settled) = settled_pending {
                    state
                        .delegations
                        .insert(delegation_id.to_owned(), settled.clone());
                    return Ok(settled);
                }
                if let Some(active) = state.active.as_ref()
                    && active.latest_delegation_id == delegation_id
                    && active.origin == origin
                {
                    let receipt = TurnReceipt {
                        delegation_id: delegation_id.to_owned(),
                        thread_id: self.session.thread_id().to_owned(),
                        turn_id: active.turn_id.clone(),
                    };
                    state
                        .delegations
                        .insert(delegation_id.to_owned(), receipt.clone());
                    return Ok(receipt);
                }
                Err(error)
            }
        }
    }

    pub(crate) async fn cancel_current(&self) -> Result<bool> {
        let turn_id = {
            let mut state = self.state.lock().await;
            let Some(active) = state.active.as_mut() else {
                return Ok(false);
            };
            if active.cancelling {
                return Ok(true);
            }
            // Fence new steering before the RPC, but do not hold the lifecycle lock while Codex
            // answers. A terminal event may legitimately race the interrupt response and remains
            // the authoritative settlement for the turn.
            active.cancelling = true;
            active.turn_id.clone()
        };
        let interrupted = self
            .session
            .interrupt(self.session.generation(), &turn_id)
            .await
            .context("interrupt Voice Analyst turn");
        if let Err(error) = interrupted {
            let mut state = self.state.lock().await;
            if let Some(active) = state.active.as_mut()
                && active.turn_id == turn_id
            {
                active.cancelling = false;
            }
            return Err(error);
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) async fn steer_voice(&self, delegation_id: &str, text: &str) -> Result<()> {
        let state = self.state.lock().await;
        let turn = state
            .delegations
            .get(delegation_id.trim())
            .ok_or_else(|| anyhow!("unknown Voice delegation: {delegation_id}"))?;
        let active = state
            .active
            .as_ref()
            .filter(|active| active.turn_id == turn.turn_id)
            .ok_or_else(|| anyhow!("Voice Analyst turn is no longer active"))?;
        if active.cancelling {
            bail!("Voice Analyst turn is cancelling");
        }
        self.session
            .steer(self.session.generation(), &active.turn_id, text)
            .await
            .context("steer Voice Analyst turn")
    }

    #[cfg(test)]
    pub(crate) async fn cancel_voice(&self, delegation_id: &str) -> Result<bool> {
        {
            let state = self.state.lock().await;
            let turn = state
                .delegations
                .get(delegation_id.trim())
                .ok_or_else(|| anyhow!("unknown Voice delegation: {delegation_id}"))?;
            if state
                .active
                .as_ref()
                .is_none_or(|active| active.turn_id != turn.turn_id)
            {
                bail!("Voice Analyst turn is no longer active");
            }
        }
        self.cancel_current().await
    }

    pub(crate) async fn is_busy(&self) -> bool {
        let state = self.state.lock().await;
        state.active.is_some() || state.pending.is_some()
    }
}

fn active_from(receipt: &TurnReceipt, origin: AnalystTurnOrigin) -> ActiveTurn {
    ActiveTurn {
        turn_id: receipt.turn_id.clone(),
        latest_delegation_id: receipt.delegation_id.clone(),
        origin,
        cancelling: false,
        deltas: String::new(),
        completed_text: String::new(),
    }
}

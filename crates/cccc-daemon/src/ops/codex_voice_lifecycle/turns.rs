use super::turn_wait::for_settlement as wait_for_turn_settlement;
use super::*;
#[cfg(test)]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};

const MANAGED_CANCEL_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl AnalystLifecycle {
    pub(crate) async fn admit_voice(
        &self,
        delegation_id: &str,
        text: &str,
    ) -> Result<VoiceDelegationAdmission> {
        self.admit_input(delegation_id, text, AnalystTurnOrigin::Voice)
            .await
    }

    pub(crate) async fn begin_actor_result(
        &self,
        correlation_id: &str,
        text: &str,
        speakable: bool,
    ) -> Result<VoiceDelegationAdmission> {
        self.admit_input(
            correlation_id,
            text,
            AnalystTurnOrigin::ActorResult { speakable },
        )
        .await
    }

    pub(super) async fn admit_input(
        &self,
        delegation_id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
    ) -> Result<VoiceDelegationAdmission> {
        let delegation_id = delegation_id.trim();
        let text = text.trim();
        if delegation_id.is_empty() || text.is_empty() {
            bail!("Voice delegation id and text are required");
        }
        let mut state = self.state.lock().await;
        if state.invalidated {
            bail!("Voice Analyst lifecycle is no longer trustworthy");
        }
        if self.session.structured_only() {
            // Pure ACP has no native busy-input surface. Every accepted input
            // uses a visible FIFO, never an overlapping session/prompt request.
            return self.enqueue(delegation_id, text, origin, state);
        }
        if let Some(receipt) = state.delegations.get(delegation_id) {
            return Ok(VoiceDelegationAdmission::Turn(receipt.clone()));
        }
        if state
            .native_pending
            .iter()
            .any(|pending| pending.delegation_id == delegation_id)
        {
            return Ok(VoiceDelegationAdmission::NativeInputPending);
        }
        if let Some(turn_id) = state
            .active
            .as_ref()
            .filter(|active| !active.cancelling && self.session.supports_steer())
            .map(|active| active.turn_id.clone())
        {
            match self
                .session
                .steer(self.session.generation(), &turn_id, text)
                .await
            {
                Ok(()) => {
                    let active = state.active.as_mut().expect("locked active turn");
                    active.latest_delegation_id = delegation_id.to_owned();
                    active.delegation_ids.push(delegation_id.to_owned());
                    active.origin = active.origin.merged(origin);
                    let receipt = TurnReceipt {
                        delegation_id: delegation_id.to_owned(),
                        thread_id: self.session.thread_id().to_owned(),
                        turn_id,
                    };
                    state
                        .delegations
                        .insert(delegation_id.to_owned(), receipt.clone());
                    let _ = self.events.send(AnalystLifecycleEvent::Associated {
                        receipt: receipt.clone(),
                        origin,
                    });
                    return Ok(VoiceDelegationAdmission::Turn(receipt));
                }
                Err(error) if steer_rejection_can_use_native_input(&error) => {
                    tracing::debug!(%error, "exact Voice Analyst steer was rejected; using native Runtime input");
                }
                Err(error) => return Err(error).context("steer active Voice Analyst delegation"),
            }
        }
        if state.active.is_some() || state.pending.is_some() || !state.native_pending.is_empty() {
            return self
                .register_native_input(delegation_id, text, origin, state)
                .await;
        }
        match self.start_new(delegation_id, text, origin, state).await {
            Ok(receipt) => Ok(VoiceDelegationAdmission::Turn(receipt)),
            Err(error) if is_would_block(&error) => {
                let state = self.state.lock().await;
                self.register_native_input(delegation_id, text, origin, state)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn register_native_input(
        &self,
        delegation_id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
        mut state: tokio::sync::MutexGuard<'_, LifecycleState>,
    ) -> Result<VoiceDelegationAdmission> {
        state.native_pending.push_back(PendingStart {
            delegation_id: delegation_id.to_owned(),
            origin,
            cancelling: false,
        });
        drop(state);
        if let Err(error) = self
            .session
            .register_native_input(self.session.generation(), delegation_id, text)
            .await
        {
            let mut state = self.state.lock().await;
            remove_native_pending(&mut state.native_pending, delegation_id);
            return Err(error).context("register Voice delegation for native Runtime input");
        }
        Ok(VoiceDelegationAdmission::NativeInput {
            delegation_id: delegation_id.to_owned(),
            text: text.to_owned(),
        })
    }

    pub(super) async fn start_new(
        &self,
        delegation_id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
        mut state: tokio::sync::MutexGuard<'_, LifecycleState>,
    ) -> Result<TurnReceipt> {
        state.pending = Some(PendingStart {
            delegation_id: delegation_id.to_owned(),
            origin,
            cancelling: false,
        });
        state.settled_pending = None;
        let text = if self.session.structured_only() && !state.host_context_sent {
            format!(
                "{}\n\n{text}",
                super::super::codex_voice_analyst::ANALYST_INSTRUCTIONS
            )
        } else {
            text.to_owned()
        };
        drop(state);

        // A managed Runtime may publish `turn/started` before answering its start request; the
        // monitor must be able to record that authoritative event while the request is in flight.
        let result = self
            .session
            .start_turn(self.session.generation(), delegation_id, &text)
            .await
            .context("start Voice Analyst investigation");
        let mut state = self.state.lock().await;
        let cancelling = state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.delegation_id == delegation_id && pending.cancelling);
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
                state.host_context_sent = true;
                if let Some(settled) = settled_pending {
                    if settled.turn_id != receipt.turn_id {
                        bail!(
                            "managed Runtime completed a different Voice Analyst turn while starting"
                        );
                    }
                    state
                        .delegations
                        .insert(delegation_id.to_owned(), receipt.clone());
                    return Ok(receipt);
                }
                let emit_started = if let Some(active) = state.active.as_mut() {
                    if active.turn_id != receipt.turn_id {
                        bail!("managed Runtime reported two concurrent Voice Analyst turns");
                    }
                    if active.latest_delegation_id.is_empty() {
                        active.latest_delegation_id = delegation_id.to_owned();
                        active.delegation_ids.push(delegation_id.to_owned());
                    }
                    false
                } else {
                    state.active = Some(active_from(&receipt, origin));
                    state
                        .active
                        .as_mut()
                        .expect("created active turn")
                        .cancelling = cancelling;
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

    pub(crate) async fn reject_native_voice(&self, delegation_id: &str) -> Result<bool> {
        let delegation_id = delegation_id.trim();
        let removed = {
            let mut state = self.state.lock().await;
            remove_native_pending(&mut state.native_pending, delegation_id)
        };
        if !removed {
            // The Runtime may have consumed the terminal write before its submit operation
            // reported an error. In that case ownership is already authoritative and must not be
            // rolled back or replayed.
            return Ok(false);
        }
        self.session
            .forget_native_input(self.session.generation(), delegation_id)
            .await
            .context("roll back undelivered native Voice input")?;
        Ok(true)
    }

    pub(crate) async fn cancel_current(&self) -> Result<bool> {
        let wait_for_settlement = !self.session.supports_steer();
        let mut lifecycle_events = wait_for_settlement.then(|| self.subscribe());
        let (turn_id, pending_id) = {
            let mut state = self.state.lock().await;
            let queued = state.queue.drain(..).collect::<Vec<_>>();
            self.queue_len
                .store(0, std::sync::atomic::Ordering::Release);
            let had_queued = !queued.is_empty();
            for input in queued {
                self.complete_unadmitted_input(&input.delegation_id, input.origin, "cancelled", "");
            }
            if let Some(active) = state.active.as_mut() {
                if active.cancelling {
                    return Ok(true);
                }
                active.cancelling = true;
                (active.turn_id.clone(), None)
            } else if self.session.structured_only()
                && state.settled_pending.is_none()
                && let Some(pending) = state.pending.as_mut()
            {
                if pending.cancelling {
                    return Ok(true);
                }
                pending.cancelling = true;
                (String::new(), Some(pending.delegation_id.clone()))
            } else {
                return Ok(had_queued);
            }
        };
        // Correlate cancellation before admission; the protocol serializes it
        // with the exact prompt rather than holding a gate across its receipt.
        let interrupted = if let Some(id) = &pending_id {
            self.session
                .cancel_pending_input(self.session.generation(), id)
                .await
        } else {
            self.session
                .interrupt(self.session.generation(), &turn_id)
                .await
        }
        .context("interrupt Voice Analyst turn");
        if let Err(error) = interrupted {
            let mut state = self.state.lock().await;
            if let Some(active) = state.active.as_mut()
                && active.turn_id == turn_id
            {
                active.cancelling = false;
            }
            if let Some(pending) = state.pending.as_mut()
                && Some(&pending.delegation_id) == pending_id.as_ref()
            {
                pending.cancelling = false;
            }
            return Err(error);
        }
        if let Some(events) = lifecycle_events.as_mut() {
            let settled = match tokio::time::timeout(MANAGED_CANCEL_SETTLE_TIMEOUT, async {
                if let Some(id) = &pending_id {
                    super::turn_wait::for_input_settlement(events, id).await
                } else {
                    wait_for_turn_settlement(events, &turn_id).await
                }
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!(
                    "managed Runtime did not confirm turn cancellation"
                )),
            };
            if let Err(error) = settled {
                self.invalidate().await;
                if let Err(stop_error) = self.session.stop(self.session.generation()).await {
                    tracing::warn!(%stop_error, "failed to stop an untrustworthy managed Runtime after cancellation");
                }
                return Err(error);
            }
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
        state.active.is_some()
            || state.pending.is_some()
            || !state.native_pending.is_empty()
            || !state.queue.is_empty()
    }

    pub(crate) async fn terminal_input_allowed(&self) -> bool {
        // The native terminal is part of the managed session, not a competing controller. Each
        // admitted Runtime owns whether input typed while busy steers or queues; CCCC observes the
        // resulting lifecycle instead of suppressing the user's keystrokes.
        true
    }
}

fn remove_native_pending(
    pending: &mut std::collections::VecDeque<PendingStart>,
    delegation_id: &str,
) -> bool {
    let Some(index) = pending
        .iter()
        .position(|pending| pending.delegation_id == delegation_id)
    else {
        return false;
    };
    pending.remove(index);
    true
}

fn active_from(receipt: &TurnReceipt, origin: AnalystTurnOrigin) -> ActiveTurn {
    ActiveTurn {
        turn_id: receipt.turn_id.clone(),
        latest_delegation_id: receipt.delegation_id.clone(),
        delegation_ids: vec![receipt.delegation_id.clone()],
        origin,
        cancelling: false,
        deltas: String::new(),
        completed_text: String::new(),
        result_overflowed: false,
    }
}

fn steer_rejection_can_use_native_input(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Other | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::NotFound
    )
}

pub(super) fn is_would_block(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
    })
}

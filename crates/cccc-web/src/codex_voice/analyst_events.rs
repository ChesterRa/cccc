use super::*;

const MAX_PENDING_ACTOR_RESULTS: usize = 32;

impl AnalystRuntime {
    pub(super) fn start_monitor(
        self: &Arc<Self>,
        home: HomeLayout,
        ledger_events: Option<LedgerEventHub>,
    ) {
        let mut events = self.analyst.subscribe_lifecycle();
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "Voice Analyst runtime projection fell behind; invalidating its derived state"
                        );
                        if let Some(runtime) = weak.upgrade() {
                            runtime.mark_failed("analyst_event_gap");
                        }
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let Some(runtime) = weak.upgrade() else {
                    break;
                };
                match event {
                    AnalystLifecycleEvent::Started { .. } => {
                        runtime.mark_working();
                        if let Err(error) = persistence::persist_analyst(&home, &runtime, true) {
                            tracing::warn!(%error, "failed to persist materialized Voice Analyst");
                        }
                    }
                    AnalystLifecycleEvent::Associated { .. } => runtime.mark_working(),
                    AnalystLifecycleEvent::Completed { status, result, .. } => {
                        if status == "completed" && !result.trim().is_empty() {
                            runtime.mark_result(&result);
                        } else {
                            runtime.mark_ready();
                        }
                        runtime.try_start_pending_actor_result().await;
                    }
                    AnalystLifecycleEvent::NeedsAttention { code } => runtime.mark_failed(code),
                    AnalystLifecycleEvent::Disconnected => {
                        runtime.mark_failed("analyst_disconnected");
                    }
                    AnalystLifecycleEvent::TrackedWork(work) => {
                        if let Some(events) = ledger_events.clone() {
                            runtime.track_work(home.clone(), events, work);
                        }
                    }
                    AnalystLifecycleEvent::Progress { .. } => {}
                }
            }
        });
        *self
            .monitor
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(task);
    }

    fn track_work(
        self: &Arc<Self>,
        home: HomeLayout,
        events: LedgerEventHub,
        work: cccc_daemon::experimental_codex_voice::TrackedWork,
    ) {
        let Some(call_generation) = self
            .call_generation
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        else {
            return;
        };
        let tracking_key = format!(
            "{}:{}:{}",
            call_generation, work.group_id, work.source_event_id
        );
        if !self
            .tracked_work
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(tracking_key.clone())
        {
            return;
        }
        codex_voice_actor_results::spawn(
            Arc::downgrade(self),
            home,
            events,
            call_generation,
            tracking_key,
            work,
        );
    }

    pub(crate) fn finish_tracking(&self, tracking_key: &str) {
        self.tracked_work
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(tracking_key);
    }

    pub(crate) async fn accept_actor_result(
        &self,
        call_generation: &str,
        result: ObservedActorResult,
    ) {
        {
            let mut pending = self
                .pending_results
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if pending.len() >= MAX_PENDING_ACTOR_RESULTS {
                pending.pop_front();
            }
            pending.push_back((call_generation.to_owned(), result));
        }
        self.try_start_pending_actor_result().await;
    }

    async fn try_start_pending_actor_result(&self) {
        let _gate = self.actor_result_gate.lock().await;
        if self.analyst.is_busy().await {
            return;
        }
        let Some((call_generation, result)) = self
            .pending_results
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
        else {
            return;
        };
        // Results always return to the persistent Analyst. Generation only fences speech so an
        // old call can never speak into a later one.
        let speakable = self.matches_call_generation(&call_generation);
        if let Err(error) = self
            .analyst
            .begin_actor_result(&result.correlation_id, &result.prompt, speakable)
            .await
        {
            if self.analyst.is_busy().await {
                self.pending_results
                    .lock()
                    .unwrap_or_else(|lock_error| lock_error.into_inner())
                    .push_front((call_generation, result));
                return;
            }
            tracing::warn!(%error, "failed to return linked Actor result to Voice Analyst");
            self.mark_failed("actor_result_return_failed");
        }
    }
}

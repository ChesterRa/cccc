use super::*;

impl AnalystRuntime {
    pub(super) fn start_monitor(self: &Arc<Self>, home: HomeLayout) {
        let mut events = self.analyst.subscribe_lifecycle();
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "Voice Analyst runtime projection lost events");
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
                    AnalystLifecycleEvent::Completed {
                        turn_id,
                        delegation_ids,
                        status,
                        result,
                        ..
                    } => {
                        if status == "completed" && !result.trim().is_empty() {
                            runtime.mark_result(&result);
                        } else {
                            runtime.mark_ready();
                        }
                        let result = if status == "completed" {
                            result
                        } else {
                            format!(
                                "The Analyst did not complete this update ({status}). Check the source messages; do not report the work as completed."
                            )
                        };
                        if let Err(error) = cccc_core::voice_notifications::processed(
                            &home,
                            &delegation_ids,
                            runtime.analyst.generation(),
                            &turn_id,
                            &result,
                        ) {
                            tracing::warn!(%error, "failed to persist Voice notification result");
                            runtime.mark_failed("actor_result_return_failed");
                        }
                    }
                    AnalystLifecycleEvent::NeedsAttention { code } => runtime.mark_failed(code),
                    AnalystLifecycleEvent::Disconnected => {
                        runtime.mark_failed("analyst_disconnected")
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
}

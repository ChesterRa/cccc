use super::*;

impl AnalystRuntime {
    pub(super) fn start_monitor(
        self: &Arc<Self>,
        home: HomeLayout,
        history_failure: Arc<AtomicBool>,
    ) {
        let mut events = self.analyst.subscribe_lifecycle();
        let weak = Arc::downgrade(self);
        // Persist Chet facts in order without holding up Voice projection if
        // the daemon or filesystem is slow. A bounded queue makes loss visible.
        // The worker drains queued facts even after the Analyst is released.
        let (history_tx, mut history_rx) = tokio::sync::mpsc::channel(64);
        *self
            .chet_writer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(history_tx.clone());
        let history_home = home.clone();
        let history_generation = self.analyst.generation().to_owned();
        let writer_failure = Arc::clone(&history_failure);
        let writer_runtime = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut voice = std::collections::HashMap::new();
            while let Some(command) = history_rx.recv().await {
                let completion = match &command {
                    chet_events::HistoryCommand::Lifecycle(
                        event @ AnalystLifecycleEvent::Completed { .. },
                    ) => Some(event.clone()),
                    _ => None,
                };
                let result = match command {
                    chet_events::HistoryCommand::Lifecycle(event) => {
                        chet_events::persist_event(
                            &history_home,
                            &history_generation,
                            &event,
                            &voice,
                        )
                        .await
                    }
                    chet_events::HistoryCommand::Voice { key, delegation } => {
                        chet_events::persist_voice(
                            &history_home,
                            &history_generation,
                            &key,
                            &delegation,
                            &mut voice,
                        )
                        .await
                    }
                };
                if result.is_err() {
                    writer_failure.store(true, Ordering::Release);
                    tracing::warn!("Chet lifecycle history could not be confirmed");
                }
                // Keep a confirmed terminal event live until its fact is
                // durable or a write failure is visible. An unconfirmed
                // delivery intentionally has no terminal fact to persist.
                if let (Some(runtime), Some(event)) = (writer_runtime.upgrade(), completion) {
                    chet_events::update_live(&runtime, &event);
                }
            }
        });
        let task = tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "Voice Analyst runtime projection lost events");
                        if let Some(runtime) = weak.upgrade() {
                            history_failure.store(true, Ordering::Release);
                            chet_events::clear_live(&runtime);
                            runtime.project_manual_event(&AnalystLifecycleEvent::NeedsAttention {
                                code: "analyst_event_gap",
                            });
                            runtime.mark_failed("analyst_event_gap");
                        }
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        if let Some(runtime) = weak.upgrade() {
                            chet_events::clear_live(&runtime);
                        }
                        break;
                    }
                };
                let Some(runtime) = weak.upgrade() else {
                    break;
                };
                let completion = matches!(event, AnalystLifecycleEvent::Completed { .. });
                if !completion {
                    chet_events::update_live(&runtime, &event);
                }
                if chet_events::is_history_event(&event)
                    && history_tx
                        .try_send(chet_events::HistoryCommand::Lifecycle(event.clone()))
                        .is_err()
                {
                    history_failure.store(true, Ordering::Release);
                    tracing::warn!("Chet lifecycle history queue is unavailable");
                    if completion {
                        chet_events::update_live(&runtime, &event);
                    }
                }
                runtime.project_manual_event(&event);
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
                        error,
                        result,
                        ..
                    } => {
                        runtime.mark_completed(&status, &result, &error);
                        let result = if status == "completed" {
                            result
                        } else {
                            format!(
                                "The work on this update did not complete ({status}). Check the source messages; do not report it as completed."
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
                    AnalystLifecycleEvent::Progress { text, .. } => runtime.add_progress(&text),
                    AnalystLifecycleEvent::Queued { .. } => {}
                }
            }
        });
        *self
            .monitor
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(task);
    }
}

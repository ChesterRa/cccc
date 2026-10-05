use super::*;

impl AnalystSnapshot {
    pub(super) fn reusable_for_call(&self) -> bool {
        !(self.phase == "needs_attention"
            && matches!(
                self.warning.as_str(),
                "analyst_disconnected" | "analyst_event_gap"
            ))
    }
}

impl AnalystSnapshot {
    pub(super) fn complete(&mut self, status: &str, result: &str, error: &str) {
        let invalidated = !self.reusable_for_call();
        if !invalidated {
            self.phase = "ready".into();
            self.warning.clear();
        }
        self.last_result = result.trim().to_owned();
        self.progress.clear();
        self.last_error.clear();
        if !matches!(status, "completed" | "cancelled") {
            if !invalidated {
                self.phase = "needs_attention".into();
                self.warning = "analyst_turn_failed".into();
            }
            self.last_error = if error.trim().is_empty() {
                status
            } else {
                error
            }
            .to_owned();
        }
    }
}

impl AnalystRuntime {
    pub(super) fn new(
        workdir: PathBuf,
        analyst: CodexVoiceAnalyst,
        launch_runtime: ResolvedAgentRuntime,
        phase: &str,
        warning: String,
    ) -> Self {
        Self {
            workdir,
            analyst: Arc::new(analyst),
            launch_runtime,
            terminal_gate: Mutex::new(()),
            snapshot: StdMutex::new(AnalystSnapshot {
                phase: phase.to_owned(),
                last_result: String::new(),
                warning,
                progress: String::new(),
                last_error: String::new(),
                manual_tasks: Vec::new(),
                manual_task_id: None,
            }),
            monitor: StdMutex::new(None),
        }
    }

    pub(super) fn reusable_for_call(&self) -> bool {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reusable_for_call()
    }

    pub(super) fn matches_launch(&self, fingerprint: [u8; 32]) -> bool {
        self.launch_runtime.fingerprint() == fingerprint
    }

    pub(super) fn launch_runtime(&self) -> ResolvedAgentRuntime {
        self.launch_runtime.clone()
    }

    pub(crate) fn analyst(&self) -> Arc<CodexVoiceAnalyst> {
        Arc::clone(&self.analyst)
    }

    pub(crate) fn info(&self) -> AnalystInfo {
        let snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        AnalystInfo {
            generation: self.analyst.generation().to_owned(),
            tui_ready: self.analyst.tui_ready(),
            phase: snapshot.phase.clone(),
            last_result: snapshot.last_result.clone(),
            warning: snapshot.warning.clone(),
            structured: self.analyst.structured_only(),
            queued_inputs: self.analyst.queued_inputs(),
            permissions: self.analyst.permissions(),
            progress: snapshot.progress.clone(),
            last_error: snapshot.last_error.clone(),
            manual_tasks: snapshot.manual_tasks.clone(),
            manual_task_id: snapshot.manual_task_id.clone(),
        }
    }

    pub(super) fn mark_working(&self) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "working".into();
        snapshot.warning.clear();
        snapshot.progress.clear();
        snapshot.last_error.clear();
        snapshot.last_result.clear();
    }

    pub(super) fn mark_completed(&self, status: &str, result: &str, error: &str) {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .complete(status, result, error);
    }

    pub(super) fn mark_failed(&self, warning: &str) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "needs_attention".into();
        snapshot.warning = warning.trim().to_owned();
    }

    pub(super) fn add_progress(&self, text: &str) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if snapshot.progress.len().saturating_add(text.len()) <= 32 * 1024 {
            snapshot.progress.push_str(text);
        }
    }
}

impl Drop for AnalystRuntime {
    fn drop(&mut self) {
        if let Ok(mut monitor) = self.monitor.lock()
            && let Some(task) = monitor.take()
        {
            task.abort();
        }
    }
}

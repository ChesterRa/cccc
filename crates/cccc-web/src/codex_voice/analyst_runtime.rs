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
            }),
            monitor: StdMutex::new(None),
            call_generation: StdMutex::new(None),
            tracked_work: StdMutex::new(HashSet::new()),
            pending_results: StdMutex::new(VecDeque::new()),
            actor_result_gate: Mutex::new(()),
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
        }
    }

    pub(super) fn mark_working(&self) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "working".into();
        snapshot.warning.clear();
    }

    pub(super) fn mark_ready(&self) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "ready".into();
    }

    pub(super) fn mark_result(&self, result: &str) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "ready".into();
        snapshot.last_result = result.trim().to_owned();
        snapshot.warning.clear();
    }

    pub(super) fn mark_failed(&self, warning: &str) {
        let mut snapshot = self
            .snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        snapshot.phase = "needs_attention".into();
        snapshot.warning = warning.trim().to_owned();
    }

    pub(super) fn set_call_generation(&self, generation: Option<&str>) {
        *self
            .call_generation
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = generation.map(str::to_owned);
    }

    pub(crate) fn matches_call_generation(&self, generation: &str) -> bool {
        let call_generation = self
            .call_generation
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        actor_result_is_speakable(call_generation.as_deref(), generation)
    }
}

pub(super) fn actor_result_is_speakable(
    active_generation: Option<&str>,
    source_generation: &str,
) -> bool {
    active_generation == Some(source_generation)
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

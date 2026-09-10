use serde_json::Value;
use std::time::Duration;
use tokio::time::Instant;

pub(super) struct CheckpointSchedule {
    window: Option<Duration>,
    last_flush: Instant,
}

impl CheckpointSchedule {
    pub(super) fn new(assistant: &Value) -> Self {
        Self::starting_at(assistant, Instant::now())
    }

    pub(super) fn starting_at(assistant: &Value, started: Instant) -> Self {
        let raw = assistant.pointer("/config/auto_document_max_window_seconds");
        let disabled = raw.is_some_and(Value::is_null)
            || assistant.pointer("/config/auto_document_enabled") == Some(&Value::Bool(false));
        let seconds = raw
            .and_then(Value::as_f64)
            .unwrap_or(300.0)
            .clamp(10.0, 300.0);
        Self {
            window: (!disabled).then(|| Duration::from_secs_f64(seconds)),
            last_flush: started,
        }
    }

    pub(super) fn due(&self) -> bool {
        self.window
            .is_some_and(|window| self.last_flush.elapsed() >= window)
    }

    pub(super) fn flushed(&mut self) {
        self.last_flush = Instant::now();
    }
}

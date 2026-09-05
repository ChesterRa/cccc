//! Lifecycle diagnostics intentionally exclude commands, credentials and RPC payloads.
use std::{future::Future, io, time::Instant};

struct Phase {
    name: &'static str,
    started: Instant,
}

impl Phase {
    fn start(name: &'static str) -> Self {
        tracing::info!(
            phase = name,
            lifecycle_event = "started",
            "managed runtime phase"
        );
        Self {
            name,
            started: Instant::now(),
        }
    }

    fn finish(self, success: bool) {
        tracing::info!(
            phase = self.name,
            lifecycle_event = "completed",
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            success,
            "managed runtime phase"
        );
    }
}

pub(super) async fn run<T>(
    name: &'static str,
    work: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    let phase = Phase::start(name);
    let result = work.await;
    phase.finish(result.is_ok());
    result
}

pub(crate) fn run_sync<T>(
    name: &'static str,
    work: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let phase = Phase::start(name);
    let result = work();
    phase.finish(result.is_ok());
    result
}

pub(super) fn request_phase(method: &str) -> Option<&'static str> {
    match method {
        "initialize" => Some("codex.initialize"),
        "thread/start" => Some("codex.thread_start"),
        "thread/resume" => Some("codex.thread_resume"),
        "thread/name/set" => Some("codex.thread_name"),
        "thread/read" => Some("codex.thread_read"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "lifecycle_timing_tests.rs"]
mod tests;

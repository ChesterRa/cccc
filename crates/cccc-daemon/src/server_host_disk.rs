//! Passive, process-owned diagnostics for the volume containing CCCC_HOME.
//! No Group, ledger, NoticeState or delivery policy participates in sampling.
use cccc_core::HomeLayout;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
const LOW_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const RECOVERY_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
struct Space {
    available_bytes: u64,
    total_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Diagnostic {
    state: &'static str,
    sample_interval_seconds: u64,
    low_bytes: u64,
    recovery_bytes: u64,
    attempts: u64,
    last_attempt_at: Option<String>,
    sampled_at: Option<String>,
    space: Option<Space>,
    error: Option<String>,
    #[serde(skip)]
    low: bool,
}

impl Default for Diagnostic {
    fn default() -> Self {
        Self {
            state: "unknown",
            sample_interval_seconds: SAMPLE_INTERVAL.as_secs(),
            low_bytes: LOW_BYTES,
            recovery_bytes: RECOVERY_BYTES,
            attempts: 0,
            last_attempt_at: None,
            sampled_at: None,
            space: None,
            error: None,
            low: false,
        }
    }
}

impl Diagnostic {
    fn record(&mut self, result: io::Result<Space>) {
        let now = cccc_contracts::utc_now();
        self.attempts = self.attempts.saturating_add(1);
        self.last_attempt_at = Some(now.clone());
        match result {
            Ok(space) => {
                self.low = space.available_bytes
                    < if self.low {
                        self.recovery_bytes
                    } else {
                        self.low_bytes
                    };
                self.state = if self.low { "low" } else { "healthy" };
                self.space = Some(space);
                self.sampled_at = Some(now);
                self.error = None;
            }
            Err(error) => {
                self.state = "unavailable";
                self.error = Some(error.to_string());
                // Preserve the prior sample, its timestamp and hysteresis.
                // A failed observation is neither recovery nor fresh capacity.
            }
        }
    }
}

type Shared = Arc<Mutex<Diagnostic>>;
type Registry = BTreeMap<PathBuf, Weak<Mutex<Diagnostic>>>;

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// Read the existing sample only. Polling never probes a filesystem.
pub(crate) fn snapshot(home: &HomeLayout) -> Option<Diagnostic> {
    let shared = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(home.root())?
        .upgrade()?;
    let result = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    Some(result)
}

fn sample(path: &Path) -> io::Result<Space> {
    let usage = fs2::statvfs(path)?;
    let space = Space {
        available_bytes: usage.available_space(),
        total_bytes: usage.total_space(),
    };
    if space.total_bytes == 0 || space.available_bytes > space.total_bytes {
        return Err(io::Error::other("invalid filesystem capacity sample"));
    }
    Ok(space)
}

pub(crate) struct HostDiskService {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl HostDiskService {
    pub(crate) fn start(home: HomeLayout) -> Self {
        Self::with_interval(home, SAMPLE_INTERVAL)
    }

    fn with_interval(home: HomeLayout, period: Duration) -> Self {
        let shared: Shared = Arc::new(Mutex::new(Diagnostic::default()));
        {
            let mut entries = registry()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            entries.retain(|_, state| state.strong_count() > 0);
            entries.insert(home.root().to_owned(), Arc::downgrade(&shared));
        }
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    _ = interval.tick() => {},
                }
                let path = home.root().to_owned();
                let result = tokio::task::spawn_blocking(move || sample(&path))
                    .await
                    .unwrap_or_else(|error| Err(io::Error::other(error.to_string())));
                shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record(result);
            }
        });
        Self {
            stop: Some(stop),
            task,
        }
    }

    pub(crate) async fn finish(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if tokio::time::timeout(Duration::from_millis(500), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for HostDiskService {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
#[path = "server_host_disk_tests.rs"]
mod tests;

use anyhow::Result;
use cccc_core::HomeLayout;
use std::fs::File;

use crate::paths::DaemonPaths;

pub struct DaemonLifecycle {
    pub paths: DaemonPaths,
    lock: Option<File>,
    active: bool,
}

impl DaemonLifecycle {
    pub fn new(paths: DaemonPaths, lock: File) -> Self {
        Self {
            paths,
            lock: Some(lock),
            active: true,
        }
    }

    pub fn finish(&mut self, result: Result<()>) -> Result<()> {
        let stop_result = self.cleanup();
        if let Err(error) = stop_result {
            if result.is_ok() {
                return Err(error);
            }
            tracing::warn!(%error, "failed to stop every runtime during daemon shutdown");
        }
        result
    }

    fn cleanup(&mut self) -> Result<Vec<cccc_runtime::SessionStatus>> {
        if !self.active {
            return Ok(Vec::new());
        }
        self.active = false;
        let result = stop_every_runtime(&self.paths.home);
        cleanup_stale(&self.paths);
        self.lock.take();
        result
    }
}

/// Gracefully stop every runtime this daemon started, closing the start gate.
/// Forced launcher exit uses cccc_runtime::force_terminate_owned instead: this
/// path may wait for protocol closure, session locks and output draining.
pub fn stop_every_runtime(home: &HomeLayout) -> Result<Vec<cccc_runtime::SessionStatus>> {
    let _ = crate::runtime_start_gate::prevent(home);
    crate::ops::actor_delivery::shutdown_all();
    let managed = crate::ops::local_headless::stop_all();
    let runtimes = crate::ops::actor_runtime::stop_all();
    match (managed, runtimes) {
        (Ok(()), Ok(statuses)) => Ok(statuses),
        (Err(error), Ok(_)) => Err(error.into()),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(managed), Err(runtimes)) => Err(anyhow::anyhow!(
            "{managed}; native runtime cleanup also failed: {runtimes}"
        )),
    }
}

impl Drop for DaemonLifecycle {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(%error, "failed to stop every runtime during cancelled daemon shutdown");
        }
    }
}

pub fn cleanup_stale(paths: &DaemonPaths) {
    for path in [&paths.socket, &paths.address, &paths.pid] {
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), %error, "failed to remove daemon state");
        }
    }
}

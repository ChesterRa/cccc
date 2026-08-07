use anyhow::Result;
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
                return Err(error.into());
            }
            tracing::warn!(%error, "failed to stop every runtime during daemon shutdown");
        }
        result
    }

    fn cleanup(&mut self) -> Result<Vec<cccc_runtime::SessionStatus>, cccc_runtime::RuntimeError> {
        if !self.active {
            return Ok(Vec::new());
        }
        self.active = false;
        let _ = crate::runtime_start_gate::prevent(&self.paths.home);
        crate::ops::actor_delivery::shutdown_all();
        crate::ops::local_headless::stop_all();
        let result = crate::ops::actor_runtime::stop_all();
        cleanup_stale(&self.paths);
        self.lock.take();
        result
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

use super::{Session, block_on_managed, managed_runtime, output};
use cccc_contracts::utc_now;
use serde_json::Value;
use std::io;
use std::sync::atomic::Ordering;
use tracing::Instrument;

impl Session {
    pub(super) fn running(&self) -> bool {
        if self.stopped.load(Ordering::Acquire) {
            return false;
        }
        self.managed.process_running()
            && (!self.has_terminal.load(Ordering::Acquire)
                || cccc_runtime::status(&self.group_id, &self.actor_id)
                    .is_ok_and(|status| status.running))
    }

    pub(super) fn stop(&self) -> io::Result<bool> {
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(false);
        }
        block_on_managed(self.managed.stop(self.managed.generation()).instrument(
            tracing::info_span!("actor_runtime_stop", group_id = %self.group_id, actor_id = %self.actor_id),
        ))?;
        if self.has_terminal.load(Ordering::Acquire) {
            match cccc_runtime::stop(&self.group_id, &self.actor_id) {
                Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        self.stopped.store(true, Ordering::Release);
        self.set_status("stopped", None);
        output::emit(self, "headless.session.stopped", serde_json::Map::new());
        Ok(true)
    }

    pub(super) fn stop_after_process_exit(&self) -> bool {
        // A dead observer is not proof that the provider job stopped. Use the
        // same confirmed stop path as actor_stop and retain ownership on error.
        match self.stop() {
            Ok(first) => first,
            Err(error) => {
                self.set_status("error", None);
                tracing::error!(
                    %error,
                    group_id = %self.group_id,
                    actor_id = %self.actor_id,
                    "failed to stop disconnected managed Actor; stop remains retryable"
                );
                false
            }
        }
    }

    pub(super) fn set_status(&self, status: &str, task_id: Option<String>) {
        if let Ok(mut state) = self.status.lock() {
            state.status = status.to_owned();
            state.task_id = task_id;
            state.updated_at = utc_now();
            if status == "stopped" {
                state.pid = None;
            }
        }
    }

    pub(super) fn attach_terminal(&self, pid: Option<u32>) {
        self.has_terminal.store(true, Ordering::Release);
        if let Ok(mut state) = self.status.lock() {
            state.pid = pid;
            state.updated_at = utc_now();
        }
    }

    pub(super) fn has_terminal(&self) -> bool {
        self.has_terminal.load(Ordering::Acquire)
    }

    pub(super) fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        managed_runtime().block_on(self.managed.respond_error(id, error))
    }
}

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

    pub(super) fn stop_after_process_exit(&self, provider_absent: bool) -> bool {
        // A dead observer is not proof that the provider job stopped: a job
        // that may still be working is stopped through the same confirmed
        // path as actor_stop, retaining ownership on error. Only a job its
        // supervisor positively reports gone has nothing left to stop.
        let result = if provider_absent {
            self.release_after_provider_exit()
        } else {
            self.stop()
        };
        match result {
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

    fn release_after_provider_exit(&self) -> io::Result<bool> {
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(false);
        }
        block_on_managed(
            self.managed
                .release_provider_for_observer_exit(self.managed.generation())
                .instrument(
                    tracing::info_span!("actor_runtime_release", group_id = %self.group_id, actor_id = %self.actor_id),
                ),
        )?;
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

    /// The runtime session of a managed Actor is a viewer attachment (for
    /// Agent View, `claude attach`), not the provider job. Its exit must only
    /// detach the terminal; the provider lifecycle is decided elsewhere.
    pub(super) fn detach_viewer(&self) {
        let _guard = match self.stop_lock.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        self.has_terminal.store(false, Ordering::Release);
        if let Ok(mut state) = self.status.lock() {
            state.pid = None;
            state.updated_at = utc_now();
        }
    }

    /// Re-open the viewer terminal after a detached attach, so deliveries and
    /// terminal views have a channel again. No-op when the session is stopped
    /// or a viewer is already attached.
    pub(super) fn reattach_viewer(&self) -> io::Result<()> {
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if self.stopped.load(Ordering::Acquire) || self.has_terminal.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(launch) = self.viewer.lock().map_err(|_| super::poisoned())?.clone() else {
            return Ok(());
        };
        let history = super::super::actor_runtime::terminal_history::config(
            &self.home,
            &self.group_id,
            &self.actor_id,
        )?;
        let status = cccc_runtime::start_with_history(
            cccc_runtime::LaunchSpec {
                group_id: self.group_id.clone(),
                actor_id: self.actor_id.clone(),
                runner: cccc_contracts::RunnerKind::Pty,
                command: launch.command,
                cwd: launch.cwd,
                env: launch.env,
                cols: 120,
                rows: 40,
            },
            history,
        )
        .map_err(io::Error::other)?;
        self.attach_terminal(status.pid);
        output::emit(
            self,
            "headless.session.viewer_attached",
            serde_json::Map::new(),
        );
        Ok(())
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

use super::{Session, block_on_managed, managed_runtime, output};
use cccc_contracts::{Actor, ActorRuntime, utc_now};
use serde_json::Value;
use std::io;
use std::sync::atomic::Ordering;
use tracing::Instrument;

/// Hermes forwarded a terminal Ctrl+C even when the terminal stopped 7 ms
/// later; this fixed pause leaves margin without reading the screen.
const HERMES_TERMINAL_INTERRUPT_GRACE: std::time::Duration = std::time::Duration::from_millis(300);
/// A terminal that stopped reading, or input already holding it, gets no
/// longer than this to take the Ctrl+C; stop then closes it anyway.
const HERMES_TERMINAL_INTERRUPT_WRITE: std::time::Duration = std::time::Duration::from_millis(500);

impl Session {
    pub(super) fn running(&self) -> bool {
        if self.stopped.load(Ordering::Acquire) {
            return false;
        }
        // The native TUI is an attachment, not the managed provider's owner.
        // This must hold even before the exited terminal has been reaped.
        self.managed.process_running()
    }

    pub(super) fn stop(&self) -> io::Result<bool> {
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(false);
        }
        // Grok's viewer can start a replacement leader when its leader goes away.
        if self.managed.runtime() == ActorRuntime::Grok && self.has_terminal.load(Ordering::Acquire)
        {
            self.stop_grok_viewer()?;
        }
        let acp_interrupt = block_on_managed(
            self.managed
                .stop_and_report(self.managed.generation())
                .instrument(tracing::info_span!(
                    "actor_runtime_stop",
                    group_id = %self.group_id,
                    actor_id = %self.actor_id
                )),
        )?;
        crate::session_owner::confirm_detached_work_stopped(&self.group_id, &self.actor_id)?;
        // Each entry reports its own evidence: the prompt CCCC wrote through
        // ACP, and Hermes's terminal, where the user may have typed a turn.
        let mut interrupt = serde_json::Map::new();
        if let Some(acp) = acp_interrupt.as_str() {
            interrupt.insert("acp".into(), acp.into());
        }
        if self.managed.runtime() == ActorRuntime::Hermes
            && self.has_terminal.load(Ordering::Acquire)
        {
            // A turn typed in Hermes's terminal belongs to the terminal, not to
            // this ACP client. Ctrl+C asks Hermes to interrupt it; Hermes
            // forwards it even when the terminal closes milliseconds later.
            // CCCC sees no end of that turn, so it records only the request.
            // Only the viewer registered now is asked, and only for a bounded
            // time, so a stuck terminal cannot hold up the stop.
            let written =
                cccc_runtime::status(&self.group_id, &self.actor_id).is_ok_and(|viewer| {
                    viewer.running
                        && cccc_runtime::write_to_session_within(
                            &viewer,
                            b"\x03",
                            HERMES_TERMINAL_INTERRUPT_WRITE,
                        )
                        .unwrap_or(false)
                });
            let terminal = if written {
                std::thread::sleep(HERMES_TERMINAL_INTERRUPT_GRACE);
                "requested"
            } else {
                "unconfirmed"
            };
            interrupt.insert("terminal".into(), terminal.into());
        }
        if self.has_terminal.load(Ordering::Acquire) {
            match cccc_runtime::stop(&self.group_id, &self.actor_id) {
                Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        self.stopped.store(true, Ordering::Release);
        self.set_status("stopped", None);
        let mut detail = serde_json::Map::new();
        if !interrupt.is_empty() {
            detail.insert("interrupt".into(), serde_json::Value::Object(interrupt));
        }
        output::emit(self, "headless.session.stopped", detail);
        Ok(true)
    }

    fn stop_grok_viewer(&self) -> io::Result<()> {
        match cccc_runtime::status(&self.group_id, &self.actor_id) {
            Ok(viewer) => {
                // Keep the registration if this terminal cannot be stopped.
                let stopped = cccc_runtime::stop_if_started_at(
                    &self.group_id,
                    &self.actor_id,
                    &viewer.started_at,
                )
                .map_err(io::Error::other)?;
                if stopped.is_none()
                    && !matches!(
                        cccc_runtime::status(&self.group_id, &self.actor_id),
                        Err(cccc_runtime::RuntimeError::NotFound(..)),
                    )
                {
                    return Err(io::Error::other(
                        "could not confirm Grok's terminal stopped",
                    ));
                }
            }
            Err(cccc_runtime::RuntimeError::NotFound(..)) => {}
            Err(error) => return Err(io::Error::other(error)),
        }
        self.record_viewer_detached();
        Ok(())
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
        self.set_status_with_reason(status, task_id, None);
    }

    /// Set the session status and, for a non-nominal status, the provider's
    /// reported cause. A later status change clears any stale reason so a
    /// recovered turn does not keep displaying the previous failure.
    pub(super) fn set_status_with_reason(
        &self,
        status: &str,
        task_id: Option<String>,
        reason: Option<String>,
    ) {
        if let Ok(mut state) = self.status.lock() {
            state.status = status.to_owned();
            state.task_id = task_id;
            state.updated_at = utc_now();
            state.reason = reason;
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

    /// The terminal takes input as it was launched. Saved settings describe the
    /// next launch, which replaces this session.
    pub(super) fn launched(&self, actor: &Actor) -> Actor {
        let mut launched = actor.clone();
        launched.runtime = self.managed.runtime();
        if let Ok(viewer) = self.viewer.lock()
            && let Some(viewer) = viewer.as_ref()
        {
            launched.submit = viewer.submit;
        }
        launched
    }

    pub(super) fn detach_viewer(&self) -> io::Result<bool> {
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if self.stopped.load(Ordering::Acquire)
            || cccc_runtime::status(&self.group_id, &self.actor_id)
                .is_ok_and(|status| status.running)
        {
            return Ok(false);
        }
        Ok(self.record_viewer_detached())
    }

    fn record_viewer_detached(&self) -> bool {
        let detached = self.has_terminal.swap(false, Ordering::AcqRel);
        if detached && let Ok(mut status) = self.status.lock() {
            status.pid = None;
            status.updated_at = utc_now();
        }
        detached
    }

    pub(super) fn reattach_viewer(&self) -> io::Result<()> {
        self.delivery_viewer().map(drop)
    }

    /// Select the viewer while the managed generation still owns its lifecycle
    /// lock. The terminal writer will recheck this viewer's PID and start time.
    pub(super) fn delivery_viewer(&self) -> io::Result<Option<cccc_runtime::SessionStatus>> {
        if !self.managed.tui_ready() {
            return Ok(None);
        }
        let _guard = self.stop_lock.lock().map_err(|_| super::poisoned())?;
        if !self.running() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "managed provider is not running",
            ));
        }
        if let Ok(status) = cccc_runtime::status(&self.group_id, &self.actor_id)
            && status.running
        {
            return Ok(Some(status));
        }
        let viewer = self
            .viewer
            .lock()
            .map_err(|_| super::poisoned())?
            .clone()
            .ok_or_else(|| io::Error::other("managed viewer launch configuration is missing"))?;
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
                command: viewer.command,
                cwd: viewer.cwd,
                env: viewer.env,
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
        Ok(Some(status))
    }

    #[cfg(test)]
    pub(super) fn has_terminal(&self) -> bool {
        self.has_terminal.load(Ordering::Acquire)
    }

    pub(super) fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        managed_runtime().block_on(self.managed.respond_error(id, error))
    }
}

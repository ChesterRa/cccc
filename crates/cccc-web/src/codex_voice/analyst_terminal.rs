use super::*;
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

impl AnalystRuntime {
    pub(crate) async fn attach_terminal(
        &self,
        mode: cccc_runtime::TerminalAttachMode,
        takeover: bool,
        since: Option<u64>,
        prefer_snapshot: bool,
        initial_size: Option<(u16, u16)>,
    ) -> Result<cccc_runtime::TerminalAttachment> {
        let _gate = self.terminal_gate.lock().await;
        let (scope_id, session_id) = self.terminal_runtime_key();
        let running = cccc_runtime::status(&scope_id, &session_id)
            .map(|status| status.running)
            .unwrap_or(false);
        if !running {
            let command = self.analyst.tui_command();
            let root = self.root.clone();
            let scope = scope_id.clone();
            let session = session_id.clone();
            let (cols, rows) = initial_size.unwrap_or((120, 32));
            tokio::task::spawn_blocking(move || {
                cccc_runtime::start(cccc_runtime::LaunchSpec {
                    group_id: scope,
                    actor_id: session,
                    runner: cccc_contracts::RunnerKind::Pty,
                    command,
                    cwd: root,
                    env: BTreeMap::new(),
                    cols,
                    rows,
                })
            })
            .await
            .map_err(|error| anyhow!("Voice Analyst terminal task failed: {error}"))??;
        }
        match (prefer_snapshot, initial_size) {
            (true, Some((cols, rows))) => cccc_runtime::attach_with_snapshot_and_size(
                &scope_id,
                &session_id,
                mode,
                takeover,
                since,
                cols,
                rows,
            ),
            (true, None) => {
                cccc_runtime::attach_with_snapshot(&scope_id, &session_id, mode, takeover, since)
            }
            (false, Some((cols, rows))) => cccc_runtime::attach_with_size(
                &scope_id,
                &session_id,
                mode,
                takeover,
                since,
                cols,
                rows,
            ),
            (false, None) => cccc_runtime::attach(&scope_id, &session_id, mode, takeover, since),
        }
        .map_err(Into::into)
    }

    pub(crate) fn terminal_writable(&self, attachment_id: u64) -> Result<bool> {
        let (scope_id, session_id) = self.terminal_runtime_key();
        cccc_runtime::attachment_writable(&scope_id, &session_id, attachment_id).map_err(Into::into)
    }

    pub(crate) fn resize_terminal(&self, attachment_id: u64, cols: u16, rows: u16) -> Result<bool> {
        let (scope_id, session_id) = self.terminal_runtime_key();
        cccc_runtime::resize_from_attachment(&scope_id, &session_id, attachment_id, cols, rows)
            .map_err(Into::into)
    }

    pub(super) fn stop_terminal(&self) {
        let (scope_id, session_id) = self.terminal_runtime_key();
        match cccc_runtime::stop(&scope_id, &session_id) {
            Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(_, _)) => {}
            Err(error) => tracing::warn!(%error, "failed to stop Voice Analyst terminal"),
        }
    }

    fn terminal_runtime_key(&self) -> (String, String) {
        (
            format!("codex-voice-terminal:{}", self.group_id),
            self.analyst.generation().to_owned(),
        )
    }
}

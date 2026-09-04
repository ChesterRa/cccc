use super::*;
use anyhow::{Result, anyhow};
use cccc_contracts::ActorRuntime;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const TERMINAL_SUBMIT_DELAY: Duration = Duration::from_millis(1_500);
const TERMINAL_REPEAT_SUBMIT_DELAY: Duration = Duration::from_millis(200);

impl AnalystRuntime {
    pub(crate) async fn attach_terminal(
        &self,
        mode: cccc_runtime::TerminalAttachMode,
        takeover: bool,
        since: Option<u64>,
        prefer_snapshot: bool,
        initial_size: Option<(u16, u16)>,
    ) -> Result<cccc_runtime::TerminalAttachment> {
        self.ensure_terminal(initial_size).await?;
        let (scope_id, session_id) = self.terminal_runtime_key();
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

    pub(crate) async fn submit_native_voice_input(&self, text: &str) -> Result<bool> {
        self.ensure_terminal(None).await?;
        let (scope_id, session_id) = self.terminal_runtime_key();
        let runtime = self.launch_runtime.runtime;
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || {
            let cancelled = AtomicBool::new(false);
            if !cccc_runtime::wait_for_input_ready(
                &scope_id,
                &session_id,
                std::time::Duration::from_secs(15),
                &cancelled,
            )? {
                return Err(anyhow!(
                    "Voice Analyst terminal input is not ready; no input was written"
                ));
            }
            let (payload, submits) = prepared_native_input(runtime, &text, true)?;
            cccc_runtime::submit_sequence_interruptible(
                &scope_id,
                &session_id,
                payload.as_bytes(),
                submits,
                TERMINAL_SUBMIT_DELAY,
                TERMINAL_REPEAT_SUBMIT_DELAY,
                &cancelled,
            )
            .map_err(Into::into)
        })
        .await
        .map_err(|error| anyhow!("Voice Analyst native input task failed: {error}"))?
    }

    async fn ensure_terminal(&self, initial_size: Option<(u16, u16)>) -> Result<()> {
        let _gate = self.terminal_gate.lock().await;
        let (scope_id, session_id) = self.terminal_runtime_key();
        if cccc_runtime::status(&scope_id, &session_id)
            .map(|status| status.running)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let command = self.analyst.tui_command();
        let root = self.workdir.clone();
        let env = self.analyst.tui_environment();
        let (cols, rows) = initial_size.unwrap_or((120, 32));
        tokio::task::spawn_blocking(move || {
            cccc_runtime::start(cccc_runtime::LaunchSpec {
                group_id: scope_id,
                actor_id: session_id,
                runner: cccc_contracts::RunnerKind::Pty,
                command,
                cwd: root,
                env,
                cols,
                rows,
            })
        })
        .await
        .map_err(|error| anyhow!("Voice Analyst terminal task failed: {error}"))??;
        Ok(())
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
            "codex-voice-terminal".into(),
            self.analyst.generation().to_owned(),
        )
    }
}

fn prepared_native_input(
    runtime: ActorRuntime,
    text: &str,
    bracketed_paste: bool,
) -> Result<(String, &'static [&'static [u8]])> {
    let raw = text.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return Err(anyhow!("Voice Analyst native input is empty"));
    }
    let payload = if raw.contains(['\r', '\n']) && bracketed_paste {
        format!("\u{1b}[200~{raw}\u{1b}[201~")
    } else if raw.contains(['\r', '\n']) {
        return Err(anyhow!(
            "Voice Analyst terminal has not enabled safe multiline paste"
        ));
    } else {
        raw.to_owned()
    };
    let submits: &'static [&'static [u8]] = if runtime == ActorRuntime::Codex {
        &[b"\r", b"\r"]
    } else {
        &[b"\r"]
    };
    Ok((payload, submits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_voice_input_uses_the_same_runtime_submit_conventions_as_actors() {
        let (codex, submits) =
            prepared_native_input(ActorRuntime::Codex, "one line", false).expect("Codex input");
        assert_eq!(codex, "one line");
        assert_eq!(submits, &[&b"\r"[..], &b"\r"[..]]);
        assert!(prepared_native_input(ActorRuntime::Codex, "one\ntwo", false).is_err());

        let (claude, submits) =
            prepared_native_input(ActorRuntime::Claude, "one\ntwo", true).expect("Claude input");
        assert_eq!(claude, "\u{1b}[200~one\ntwo\u{1b}[201~");
        assert_eq!(submits, &[&b"\r"[..]]);
    }
}

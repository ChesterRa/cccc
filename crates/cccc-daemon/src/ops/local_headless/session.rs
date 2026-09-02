use super::{
    ActiveTurn, Session, SessionTransport, Turn, TurnOutputState, codex_runtime, output, poisoned,
    protocol,
};
use cccc_contracts::{ActorRuntime, utc_now};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

impl Session {
    pub(super) fn running(&self) -> bool {
        if self.stopped.load(Ordering::Acquire) {
            return false;
        }
        match &self.transport {
            SessionTransport::Process { child, .. } => child
                .lock()
                .ok()
                .is_some_and(|mut child| child.try_wait().ok().flatten().is_none()),
            SessionTransport::CodexApp {
                session,
                has_terminal,
            } => {
                session.process_running()
                    && (!has_terminal.load(Ordering::Acquire)
                        || cccc_runtime::status(&self.group_id, &self.actor_id)
                            .is_ok_and(|status| status.running))
            }
        }
    }

    pub(super) fn stop(&self) {
        self.stop_after_invalidate(|| {});
    }

    pub(super) fn stop_after_invalidate(&self, after_invalidate: impl FnOnce()) {
        let first_stop = !self.stopped.swap(true, Ordering::AcqRel);
        after_invalidate();
        match &self.transport {
            SessionTransport::Process { child, .. } => {
                if let Ok(mut child) = child.lock() {
                    if child.try_wait().ok().flatten().is_none() {
                        let _ = child.kill();
                    }
                    let _ = child.wait();
                }
            }
            SessionTransport::CodexApp {
                session,
                has_terminal,
            } => {
                if has_terminal.load(Ordering::Acquire) {
                    let _ = cccc_runtime::stop(&self.group_id, &self.actor_id);
                }
                let _ = codex_runtime().block_on(session.stop(session.generation()));
            }
        }
        self.set_status("stopped", None);
        self.completion.1.notify_all();
        if first_stop {
            output::emit(self, "headless.session.stopped", serde_json::Map::new());
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
        if let SessionTransport::CodexApp { has_terminal, .. } = &self.transport {
            has_terminal.store(true, Ordering::Release);
        }
        if let Ok(mut state) = self.status.lock() {
            state.pid = pid;
            state.updated_at = utc_now();
        }
    }

    pub(super) fn mark_codex_thread_materialized(&self) {
        if let SessionTransport::CodexApp { session, .. } = &self.transport {
            session.mark_thread_materialized();
        }
    }

    pub(super) fn write_json(&self, value: &Value) -> io::Result<()> {
        let SessionTransport::Process { stdin, .. } = &self.transport else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Codex app-server transport does not expose process stdin",
            ));
        };
        let mut stdin = stdin.lock().map_err(|_| poisoned())?;
        serde_json::to_writer(&mut *stdin, value).map_err(io::Error::other)?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    pub(super) fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        if let SessionTransport::CodexApp { session, .. } = &self.transport {
            return codex_runtime().block_on(session.request(method, params, timeout));
        }
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .map_err(|_| poisoned())?
            .insert(id, sender);
        if let Err(error) =
            self.write_json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        {
            self.pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id));
            return Err(error);
        }
        let response = receiver.recv_timeout(timeout).map_err(|_| {
            self.pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id));
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("headless request timed out: {method}"),
            )
        })?;
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(format!(
                "headless request failed: {error}"
            )));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub(super) fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        match &self.transport {
            SessionTransport::CodexApp { session, .. } => {
                codex_runtime().block_on(session.respond_error(id, error))
            }
            SessionTransport::Process { .. } => {
                self.write_json(&json!({"jsonrpc":"2.0","id":id,"error":error}))
            }
        }
    }
}

pub(super) fn spawn_reader(
    session: Arc<Session>,
    stdout: impl std::io::Read + Send + 'static,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!(
            "cccc-headless-out:{}:{}",
            session.group_id, session.actor_id
        ))
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(message) = serde_json::from_str::<Value>(&line) {
                    output::handle_message(&session, message);
                }
            }
            let unexpected_exit = !session.stopped.swap(true, Ordering::AcqRel);
            if unexpected_exit {
                invalidate_pending_claude_resume(
                    &session,
                    "claude headless resume process exited before completing a turn",
                );
            }
            session.set_status("stopped", None);
            session.completion.1.notify_all();
        })?;
    Ok(())
}

pub(super) fn spawn_codex_reader(
    session: Arc<Session>,
    mut events: tokio::sync::broadcast::Receiver<super::super::codex_voice_analyst::AnalystEvent>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!(
            "cccc-codex-app-out:{}:{}",
            session.group_id, session.actor_id
        ))
        .spawn(move || {
            loop {
                match codex_runtime().block_on(events.recv()) {
                    Ok(event) => {
                        let disconnected = event.message.get("method").and_then(Value::as_str)
                            == Some(super::super::codex_voice_analyst::CODEX_APP_DISCONNECTED_METHOD);
                        output::handle_message(&session, event.message);
                        if disconnected {
                            session.stop();
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            group_id = %session.group_id,
                            actor_id = %session.actor_id,
                            "Codex Actor app-server event reader fell behind; stopping the unreplayable session"
                        );
                        session.stop();
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        session.stop();
                        break;
                    }
                }
            }
        })?;
    Ok(())
}

pub(super) fn invalidate_pending_claude_resume(session: &Session, error: &str) {
    if session.runtime != ActorRuntime::Claude {
        return;
    }
    let provider_session_id = session
        .resumed_provider_session_id
        .lock()
        .ok()
        .map(|mut session_id| std::mem::take(&mut *session_id))
        .unwrap_or_default();
    if provider_session_id.is_empty() {
        return;
    }
    if let Err(persist_error) = super::super::runtime_session::mark_resume_failed(
        &session.home,
        &session.group_id,
        &session.actor_id,
        error,
    ) {
        tracing::warn!(
            error = %persist_error,
            group_id = %session.group_id,
            actor_id = %session.actor_id,
            "failed to invalidate rejected Claude resume metadata"
        );
    }
    output::emit(
        session,
        "headless.session.resume_failed",
        serde_json::Map::from_iter([
            ("provider_session_id".into(), json!(provider_session_id)),
            ("error".into(), json!(error)),
        ]),
    );
}

pub(super) fn spawn_stderr(
    stderr: impl std::io::Read + Send + 'static,
    group_id: &str,
    actor_id: &str,
) -> io::Result<()> {
    let name = format!("cccc-headless-err:{group_id}:{actor_id}");
    std::thread::Builder::new().name(name).spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::debug!(message = %line, "headless provider stderr");
        }
    })?;
    Ok(())
}

pub(super) fn spawn_worker(session: Arc<Session>, receiver: Receiver<Turn>) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!(
            "cccc-headless-turn:{}:{}",
            session.group_id, session.actor_id
        ))
        .spawn(move || {
            while session.running() {
                let Ok(turn) = receiver.recv() else { break };
                let Some(generation) = claim_turn(&session, &turn) else {
                    break;
                };
                session.set_status(
                    "working",
                    Some(turn.event_id.clone()).filter(|id| !id.is_empty()),
                );
                let result = if session.runtime == ActorRuntime::Codex {
                    protocol::submit_codex(&session, &turn)
                } else {
                    protocol::submit_claude(&session, &turn)
                };
                let Ok(turn_id) = result else {
                    output::emit_turn(&session, &turn, "headless.turn.failed", "");
                    output::release_failed_reservation(&session);
                    continue;
                };
                if let Ok(mut active_turn) = session.active_turn.lock()
                    && let Some(active_turn) = active_turn.as_mut()
                {
                    active_turn.turn_id.clone_from(&turn_id);
                }
                if let Ok(mut state) = session.status.lock()
                    && state.status == "working"
                {
                    state.task_id = Some(turn_id.clone());
                    state.updated_at = utc_now();
                }
                output::emit_turn(&session, &turn, "headless.turn.started", &turn_id);
                output::announce_turn(&session);
                let mut completed = match session.completion.0.lock() {
                    Ok(value) => value,
                    Err(_) => break,
                };
                while *completed == generation && session.running() {
                    completed = match session.completion.1.wait(completed) {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                }
            }
        })?;
    Ok(())
}

fn claim_turn(session: &Session, turn: &Turn) -> Option<u64> {
    claim_turn_when(
        &session.active_turn,
        &session.completion,
        || session.running(),
        turn,
    )
}

fn claim_turn_when(
    active_turn: &std::sync::Mutex<Option<ActiveTurn>>,
    completion: &(std::sync::Mutex<u64>, std::sync::Condvar),
    is_running: impl Fn() -> bool,
    turn: &Turn,
) -> Option<u64> {
    let mut completed = completion.0.lock().ok()?;
    loop {
        if !is_running() {
            return None;
        }
        let mut active = active_turn.lock().ok()?;
        if active.is_none() {
            *active = Some(ActiveTurn {
                event_id: turn.event_id.clone(),
                turn_id: String::new(),
                control_kind: turn.control_kind.clone(),
                external: false,
                output_state: TurnOutputState::Buffering,
                pending_messages: Vec::new(),
            });
            return Some(*completed);
        }
        drop(active);
        completed = completion.1.wait(completed).ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn queued_delivery_waits_until_an_external_turn_settles() {
        let active_turn = Arc::new(std::sync::Mutex::new(Some(ActiveTurn {
            event_id: String::new(),
            turn_id: "turn-terminal".into(),
            control_kind: String::new(),
            external: true,
            output_state: TurnOutputState::Announced,
            pending_messages: Vec::new(),
        })));
        let completion = Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new()));
        let running = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let waiting_active = Arc::clone(&active_turn);
        let waiting_completion = Arc::clone(&completion);
        let waiting_running = Arc::clone(&running);
        let worker = std::thread::spawn(move || {
            let turn = Turn {
                text: "queued delivery".into(),
                event_id: "event-1".into(),
                control_kind: String::new(),
            };
            let claimed = claim_turn_when(
                &waiting_active,
                &waiting_completion,
                || waiting_running.load(Ordering::Acquire),
                &turn,
            );
            sender.send(claimed).expect("claim result");
        });

        assert!(
            receiver.recv_timeout(Duration::from_millis(50)).is_err(),
            "delivery claimed the thread while the terminal turn was active"
        );
        active_turn.lock().expect("active turn").take();
        let mut generation = completion.0.lock().expect("completion generation");
        *generation += 1;
        drop(generation);
        completion.1.notify_all();

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("delivery claim"),
            Some(1)
        );
        let active = active_turn.lock().expect("claimed turn");
        let active = active.as_ref().expect("delivery active");
        assert_eq!(active.event_id, "event-1");
        assert!(!active.external);
        worker.join().expect("worker");
    }
}

use super::{Session, Turn, output, poisoned, protocol};
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
        self.child
            .lock()
            .ok()
            .is_some_and(|mut child| child.try_wait().ok().flatten().is_none())
    }

    pub(super) fn stop(&self) {
        let first_stop = !self.stopped.swap(true, Ordering::AcqRel);
        if let Ok(mut child) = self.child.lock() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
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

    pub(super) fn write_json(&self, value: &Value) -> io::Result<()> {
        let mut stdin = self.stdin.lock().map_err(|_| poisoned())?;
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
            session.stopped.store(true, Ordering::Release);
            session.set_status("stopped", None);
            session.completion.1.notify_all();
        })?;
    Ok(())
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
                let generation = session
                    .completion
                    .0
                    .lock()
                    .map(|value| *value)
                    .unwrap_or_default();
                if let Ok(mut active_event_id) = session.active_event_id.lock() {
                    active_event_id.clone_from(&turn.event_id);
                }
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
                    session.set_status("waiting", None);
                    output::emit_turn(&session, &turn, "headless.turn.failed", "");
                    continue;
                };
                if let Ok(mut state) = session.status.lock()
                    && state.status == "working"
                {
                    state.task_id = Some(turn_id.clone());
                    state.updated_at = utc_now();
                }
                if !turn.control {
                    output::mark_read(&session, &turn);
                }
                output::emit_turn(&session, &turn, "headless.turn.started", &turn_id);
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

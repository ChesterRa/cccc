use crate::RuntimeError;
use crate::cancellation::wait_interruptibly;
use crate::registry::{
    Key, SharedSession, completed_history, discard_completed, lookup, remember_history, sessions,
    with_session,
};
use crate::session::{ActorLaunch, LaunchSpec, Session, SessionStatus};
use crate::session_history::SessionHistory;
use crate::transcript_archive::HistoryConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod submission;
pub use submission::{
    PtyHandoff, PtyHandoffReason, submit_sequence_for_new_kilo_viewer,
    submit_sequence_for_session_interruptible,
    submit_sequence_for_session_with_evidence_interruptible, submit_sequence_interruptible,
};

type ReapCandidate = (Key, Arc<Mutex<Session>>, SessionHistory, SessionStatus);

pub fn start(spec: LaunchSpec) -> Result<SessionStatus, RuntimeError> {
    start_inner(spec, None)
}

pub fn start_with_history(
    spec: LaunchSpec,
    history: HistoryConfig,
) -> Result<SessionStatus, RuntimeError> {
    start_inner(spec, Some(history))
}

fn start_inner(
    spec: LaunchSpec,
    history: Option<HistoryConfig>,
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment(spec, history, false, None)
}

/// Launch an Actor's primary PTY. Its attribution is set before the session is
/// registered, so no reader can find the process without it.
pub fn start_actor_with_history(
    spec: LaunchSpec,
    history: HistoryConfig,
    launch: ActorLaunch,
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment(spec, Some(history), false, Some(launch))
}

/// Launch a PTY with exactly the supplied environment, apart from terminal attribution.
pub fn start_with_history_isolated_environment(
    spec: LaunchSpec,
    history: HistoryConfig,
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment(spec, Some(history), true, None)
}

fn start_inner_environment(
    spec: LaunchSpec,
    history: Option<HistoryConfig>,
    clear_environment: bool,
    launch: Option<ActorLaunch>,
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment_with_hook(spec, history, clear_environment, launch, || {})
}

#[cfg(test)]
pub(crate) fn start_actor_with_history_and_post_spawn_hook(
    spec: LaunchSpec,
    history: HistoryConfig,
    launch: ActorLaunch,
    after_spawn: impl FnOnce(),
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment_with_hook(spec, Some(history), false, Some(launch), after_spawn)
}

fn start_inner_environment_with_hook(
    spec: LaunchSpec,
    history: Option<HistoryConfig>,
    clear_environment: bool,
    launch: Option<ActorLaunch>,
    after_spawn: impl FnOnce(),
) -> Result<SessionStatus, RuntimeError> {
    let key = (spec.group_id.clone(), spec.actor_id.clone());
    remove_exited_before_start(&key)?;
    let history_cursor_floor = match completed_history(&key.0, &key.1)? {
        Some(history) => history.end_cursor()?,
        None => 0,
    };
    #[cfg(test)]
    let mut session = Session::start_with_post_spawn_hook(
        spec,
        history,
        history_cursor_floor,
        clear_environment,
        launch,
        after_spawn,
    )?;
    #[cfg(not(test))]
    let mut session = {
        let _ = after_spawn;
        Session::start_with_environment(
            spec,
            history,
            history_cursor_floor,
            clear_environment,
            launch,
        )?
    };
    let status = session.status();
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry.contains_key(&key) {
        drop(registry);
        session.stop()?;
        return Err(RuntimeError::AlreadyRunning(key.0, key.1));
    }
    discard_completed(&key)?;
    registry.insert(key, Arc::new(Mutex::new(session)));
    Ok(status)
}

fn remove_exited_before_start(key: &Key) -> Result<(), RuntimeError> {
    let Some(existing) = sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .get(key)
        .cloned()
    else {
        return Ok(());
    };
    let running = {
        let mut session = existing.lock().map_err(|_| RuntimeError::Poisoned)?;
        session.status().running
    };
    if running {
        return Err(RuntimeError::AlreadyRunning(key.0.clone(), key.1.clone()));
    }
    let history = {
        let mut session = existing.lock().map_err(|_| RuntimeError::Poisoned)?;
        session.finish_output()?;
        session.history_handle()
    };
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry
        .get(key)
        .is_some_and(|registered| Arc::ptr_eq(registered, &existing))
    {
        registry.remove(key);
        remember_history(key.clone(), history)?;
    }
    Ok(())
}

pub fn status(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    with_session(group_id, actor_id, |session| Ok(session.status()))
}

/// Explicit user release for the exact primary PTY observed in the waiting UI.
/// This writes no native key and does not affect a replacement session.
pub fn release_startup_input(
    group: &str,
    actor: &str,
    generation: &str,
) -> Result<bool, RuntimeError> {
    with_session(group, actor, |session| {
        Ok(session.release_startup_input(generation))
    })
}

/// A user retry may start a new guarded PTY after the previous one exited.
pub fn reset_exited_startup_input(group: &str, actor: &str) -> Result<(), RuntimeError> {
    match status(group, actor) {
        Ok(status) if !status.running && status.startup_input_pending => {
            stop_if_started_at(group, actor, &status.started_at)?;
            Ok(())
        }
        Ok(_) | Err(RuntimeError::NotFound(..)) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn stop(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    stop_registered(group_id, actor_id, None)?
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))
}

pub fn stop_if_started_at(
    group_id: &str,
    actor_id: &str,
    expected_started_at: &str,
) -> Result<Option<SessionStatus>, RuntimeError> {
    stop_registered(group_id, actor_id, Some(expected_started_at))
}

fn stop_registered(
    group_id: &str,
    actor_id: &str,
    expected_started_at: Option<&str>,
) -> Result<Option<SessionStatus>, RuntimeError> {
    let current = match lookup(group_id, actor_id) {
        Ok(current) => current,
        Err(RuntimeError::NotFound(..)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut session = current.lock().map_err(|_| RuntimeError::Poisoned)?;
    if expected_started_at.is_some_and(|expected| session.status().started_at != expected) {
        return Ok(None);
    }
    // A concurrent stop must still find this session until its cleanup has succeeded.
    let status = session.stop()?;
    let history = session.history_handle();
    drop(session);
    let key = (group_id.to_owned(), actor_id.to_owned());
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    if registry
        .get(&key)
        .is_some_and(|registered| Arc::ptr_eq(registered, &current))
    {
        registry.remove(&key);
        remember_history(key, history)?;
    }
    Ok(Some(status))
}

pub fn stop_all() -> Result<Vec<SessionStatus>, RuntimeError> {
    let drained = {
        let mut sessions = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
        std::mem::take(&mut *sessions)
    };
    let mut stopped = Vec::with_capacity(drained.len());
    let mut first_error = None;
    for (key, session) in drained {
        let mut session = session.lock().map_err(|_| RuntimeError::Poisoned)?;
        let result = session.stop();
        let history = session.history_handle();
        drop(session);
        remember_history(key, history)?;
        match result {
            Ok(status) => stopped.push(status),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(stopped),
    }
}

pub fn write(group_id: &str, actor_id: &str, data: &[u8]) -> Result<(), RuntimeError> {
    let session = lookup(group_id, actor_id)?;
    let gate = session
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .input_gate();
    let _guard = gate.lock().map_err(|_| RuntimeError::Poisoned)?;
    write_locked(&session, data, None, None).map(|_| ())
}

/// Writes `data` to the terminal `expected` describes, giving up `timeout`
/// after starting to wait for its input gate: neither a paste holding the
/// gate nor a program that stopped reading can hold the caller longer. A
/// write still blocked then (macOS writes block) ends in the background once
/// the terminal stops; stopping it never waits on input. `Ok(false)`: not
/// confirmed written in time, or that terminal is gone or replaced.
pub fn write_to_session_within(
    expected: &SessionStatus,
    data: &[u8],
    timeout: Duration,
) -> Result<bool, RuntimeError> {
    let deadline = std::time::Instant::now() + timeout;
    let (sender, receiver) = std::sync::mpsc::channel();
    let expected = expected.clone();
    let data = data.to_vec();
    std::thread::Builder::new()
        .name("cccc-bounded-input".into())
        .spawn(move || {
            let expired = || std::time::Instant::now() >= deadline;
            let written = (|| {
                let session = lookup(&expected.group_id, &expected.actor_id)?;
                let gate = session
                    .lock()
                    .map_err(|_| RuntimeError::Poisoned)?
                    .input_gate();
                let Some(_guard) = crate::cancellation::lock_interruptibly(&gate, &expired)? else {
                    return Ok(false);
                };
                write_locked(&session, &data, Some(&expired), Some(&expected))
            })();
            let _ = sender.send(written);
        })?;
    match receiver.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
        Ok(Err(RuntimeError::NotFound(..))) | Err(_) => Ok(false),
        Ok(written) => written,
    }
}

pub fn submit(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submit: &[u8],
    delay: Duration,
) -> Result<(), RuntimeError> {
    let cancelled = AtomicBool::new(false);
    submit_interruptible(group_id, actor_id, payload, submit, delay, &cancelled).map(|_| ())
}

pub fn submit_interruptible(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submit: &[u8],
    delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    let submits = [submit];
    submit_sequence_interruptible(
        group_id,
        actor_id,
        payload,
        &submits,
        delay,
        Duration::ZERO,
        cancelled,
    )
}

/// Wait for the native TUI to advertise its input mode, not for a turn to finish.
/// Creating a PTY alone leaves its canonical buffer liable to truncate a paste.
pub fn wait_for_input_ready(
    group_id: &str,
    actor_id: &str,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    wait_for_terminal_input(group_id, actor_id, None, timeout, cancelled)
}

/// Let the viewer paint without holding its PTY input lane. Submission checks
/// the same frame again under the lane before writing any business input.
pub fn wait_for_new_kilo_composer(
    expected: &SessionStatus,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    wait_for_terminal_input(
        &expected.group_id,
        &expected.actor_id,
        Some(expected),
        timeout,
        cancelled,
    )
}

fn wait_for_terminal_input(
    group_id: &str,
    actor_id: &str,
    new_kilo_viewer: Option<&SessionStatus>,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(false);
        }
        let status = status(group_id, actor_id)?;
        if !status.running
            || new_kilo_viewer.is_some_and(|expected| {
                expected.pid != status.pid || expected.started_at != status.started_at
            })
        {
            return Ok(false);
        }
        let ready = if new_kilo_viewer.is_some() {
            with_session(group_id, actor_id, |session| {
                session.history_handle().kilo_composer_ready()
            })?
        } else {
            crate::bracketed_paste_enabled(group_id, actor_id)?
        };
        if ready {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        if !wait_interruptibly(remaining.min(Duration::from_millis(25)), cancelled) {
            return Ok(false);
        }
    }
}

fn write_locked(
    session: &SharedSession,
    data: &[u8],
    cancelled: Option<&dyn Fn() -> bool>,
    expected: Option<&SessionStatus>,
) -> Result<bool, RuntimeError> {
    let writer = {
        let mut current = session.lock().map_err(|_| RuntimeError::Poisoned)?;
        if let Some(expected) = expected {
            let actual = current.status();
            if actual.pid != expected.pid || actual.started_at != expected.started_at {
                return Ok(false);
            }
        }
        current.input_writer()?
    };
    // The input gate preserves ordering; blocking IO must not hold the Session
    // lock required by status, stop and process cleanup. Pin this generation.
    match cancelled {
        Some(cancelled) => crate::pty_input::write_input_interruptibly(&writer, data, cancelled),
        None => crate::pty_input::write_input(&writer, data).map(|()| true),
    }
}

pub fn resize(group_id: &str, actor_id: &str, cols: u16, rows: u16) -> Result<(), RuntimeError> {
    with_session(group_id, actor_id, |session| session.resize(cols, rows))
}

pub fn reap() -> Result<Vec<SessionStatus>, RuntimeError> {
    let snapshot = sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .iter()
        .map(|(key, session)| (key.clone(), Arc::clone(session)))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (key, shared) in snapshot {
        let mut session = shared.lock().map_err(|_| RuntimeError::Poisoned)?;
        if let Some(status) = session.prepare_reap()? {
            let history = session.history_handle();
            drop(session);
            candidates.push((key, shared, history, status));
        }
    }
    commit_reaped(candidates)
}

fn commit_reaped(candidates: Vec<ReapCandidate>) -> Result<Vec<SessionStatus>, RuntimeError> {
    let mut exited = Vec::new();
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    for (key, session, history, status) in candidates {
        if registry
            .get(&key)
            .is_some_and(|registered| Arc::ptr_eq(registered, &session))
        {
            // Preserve the closed PTY's pending gate until its explicit retry or
            // removal. Delivery can observe the pause before exit reconciliation.
            if status.startup_input_pending {
                if !session
                    .lock()
                    .map_err(|_| RuntimeError::Poisoned)?
                    .report_retained_exit()
                {
                    continue;
                }
            } else {
                registry.remove(&key);
                remember_history(key, history)?;
            }
            exited.push(status);
        }
    }
    Ok(exited)
}

#[cfg(all(test, unix))]
#[path = "manager_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "manager/startup_tests.rs"]
mod startup_tests;

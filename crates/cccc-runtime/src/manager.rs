use crate::RuntimeError;
use crate::cancellation::wait_interruptibly;
use crate::registry::{
    Key, SharedSession, completed_history, discard_completed, lookup, remember_history, sessions,
    with_session,
};
use crate::session::{LaunchSpec, Session, SessionStatus};
use crate::session_history::SessionHistory;
use crate::transcript_archive::HistoryConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod submission;
pub use submission::{submit_sequence_for_session_interruptible, submit_sequence_interruptible};

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
    start_inner_environment(spec, history, false)
}

/// Launch a PTY with exactly the supplied environment, apart from terminal attribution.
pub fn start_with_history_isolated_environment(
    spec: LaunchSpec,
    history: HistoryConfig,
) -> Result<SessionStatus, RuntimeError> {
    start_inner_environment(spec, Some(history), true)
}

fn start_inner_environment(
    spec: LaunchSpec,
    history: Option<HistoryConfig>,
    clear_environment: bool,
) -> Result<SessionStatus, RuntimeError> {
    let key = (spec.group_id.clone(), spec.actor_id.clone());
    remove_exited_before_start(&key)?;
    let history_cursor_floor = match completed_history(&key.0, &key.1)? {
        Some(history) => history.end_cursor()?,
        None => 0,
    };
    let mut session = if clear_environment {
        Session::start_with_environment(spec, history, history_cursor_floor, true)?
    } else {
        Session::start_with_history(spec, history, history_cursor_floor)?
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

pub fn stop(group_id: &str, actor_id: &str) -> Result<SessionStatus, RuntimeError> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let session = sessions()
        .write()
        .map_err(|_| RuntimeError::Poisoned)?
        .remove(&key)
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?;
    let mut session = session.lock().map_err(|_| RuntimeError::Poisoned)?;
    let status = session.stop()?;
    let history = session.history_handle();
    drop(session);
    remember_history(key, history)?;
    Ok(status)
}

pub fn stop_if_started_at(
    group_id: &str,
    actor_id: &str,
    expected_started_at: &str,
) -> Result<Option<SessionStatus>, RuntimeError> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let Ok(current) = lookup(group_id, actor_id) else {
        return Ok(None);
    };
    let mut session = current.lock().map_err(|_| RuntimeError::Poisoned)?;
    if session.status().started_at != expected_started_at {
        return Ok(None);
    }
    let status = session.stop()?;
    let history = session.history_handle();
    drop(session);
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
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(false);
        }
        if !status(group_id, actor_id)?.running {
            return Ok(false);
        }
        if crate::bracketed_paste_enabled(group_id, actor_id)? {
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
    cancelled: Option<&AtomicBool>,
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
        Some(cancelled) => crate::pty_input::write_input_interruptibly(&writer, data, &|| {
            cancelled.load(Ordering::Acquire)
        }),
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
    let mut remove = Vec::new();
    for (key, shared) in snapshot {
        let mut session = shared.lock().map_err(|_| RuntimeError::Poisoned)?;
        let status = session.status();
        if !status.running {
            session.finish_output()?;
            let history = session.history_handle();
            drop(session);
            remove.push((key, shared, history, status));
        }
    }
    commit_reaped(remove)
}

fn commit_reaped(remove: Vec<ReapCandidate>) -> Result<Vec<SessionStatus>, RuntimeError> {
    let mut exited = Vec::new();
    let mut registry = sessions().write().map_err(|_| RuntimeError::Poisoned)?;
    for (key, session, history, status) in remove {
        if registry
            .get(&key)
            .is_some_and(|registered| Arc::ptr_eq(registered, &session))
        {
            registry.remove(&key);
            remember_history(key, history)?;
            exited.push(status);
        }
    }
    Ok(exited)
}

#[cfg(all(test, unix))]
#[path = "manager_tests.rs"]
mod tests;

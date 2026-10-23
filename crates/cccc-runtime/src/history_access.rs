use crate::RuntimeError;
use crate::output::HistoryPage;
use crate::registry::{completed_history, lookup, with_session};
use crate::session_history::OutputObservation;

pub fn history(
    group_id: &str,
    actor_id: &str,
    before: Option<u64>,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match lookup(group_id, actor_id) {
        Ok(session) => session
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .history(before, limit),
        Err(RuntimeError::NotFound(_, _)) => completed_history(group_id, actor_id)?
            .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?
            .page(before, limit),
        Err(error) => Err(error),
    }
}

pub fn retained_history(group_id: &str, actor_id: &str) -> Result<HistoryPage, RuntimeError> {
    match lookup(group_id, actor_id) {
        Ok(session) => session
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .history_handle()
            .retained_page(),
        Err(RuntimeError::NotFound(_, _)) => completed_history(group_id, actor_id)?
            .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?
            .retained_page(),
        Err(error) => Err(error),
    }
}

pub fn retained_history_tail(
    group_id: &str,
    actor_id: &str,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match lookup(group_id, actor_id) {
        Ok(session) => session
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .history_handle()
            .retained_tail_page(limit),
        Err(RuntimeError::NotFound(_, _)) => completed_history(group_id, actor_id)?
            .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?
            .retained_tail_page(limit),
        Err(error) => Err(error),
    }
}

pub fn active_history_since(
    group_id: &str,
    actor_id: &str,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    lookup(group_id, actor_id)?
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .history_handle()
        .active_page_since(after, limit)
}

/// When the live session's process last wrote to its terminal, or `None` before
/// its first output or while transcript access is busy. A new session starts
/// without an observation.
pub fn last_output(
    group_id: &str,
    actor_id: &str,
) -> Result<Option<OutputObservation>, RuntimeError> {
    lookup(group_id, actor_id)?
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .history_handle()
        .last_output()
}

pub fn active_history_replay(
    group_id: &str,
    actor_id: &str,
    after: u64,
    end_cursor: Option<u64>,
    limit: usize,
) -> Result<(HistoryPage, u64), RuntimeError> {
    lookup(group_id, actor_id)?
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .history_handle()
        .active_replay_page(after, end_cursor, limit)
}

pub fn history_since(
    group_id: &str,
    actor_id: &str,
    after: u64,
    limit: usize,
) -> Result<HistoryPage, RuntimeError> {
    match lookup(group_id, actor_id) {
        Ok(session) => session
            .lock()
            .map_err(|_| RuntimeError::Poisoned)?
            .history_since(after, limit),
        Err(RuntimeError::NotFound(_, _)) => completed_history(group_id, actor_id)?
            .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))?
            .page_since(after, limit),
        Err(error) => Err(error),
    }
}

pub fn clear(group_id: &str, actor_id: &str) -> Result<(), RuntimeError> {
    with_session(group_id, actor_id, |session| session.clear())
}

pub fn bracketed_paste_enabled(group_id: &str, actor_id: &str) -> Result<bool, RuntimeError> {
    with_session(group_id, actor_id, |session| {
        session.bracketed_paste_enabled()
    })
}

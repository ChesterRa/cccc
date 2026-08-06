use crate::RuntimeError;
use crate::session::Session;
use crate::session_history::SessionHistory;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub(crate) type Key = (String, String);
pub(crate) type SharedSession = Arc<Mutex<Session>>;

pub(crate) fn sessions() -> &'static RwLock<HashMap<Key, SharedSession>> {
    static SESSIONS: OnceLock<RwLock<HashMap<Key, SharedSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn lookup(group_id: &str, actor_id: &str) -> Result<SharedSession, RuntimeError> {
    sessions()
        .read()
        .map_err(|_| RuntimeError::Poisoned)?
        .get(&(group_id.to_owned(), actor_id.to_owned()))
        .cloned()
        .ok_or_else(|| RuntimeError::NotFound(group_id.into(), actor_id.into()))
}

pub(crate) fn with_session<T>(
    group_id: &str,
    actor_id: &str,
    operation: impl FnOnce(&mut Session) -> Result<T, RuntimeError>,
) -> Result<T, RuntimeError> {
    let session = lookup(group_id, actor_id)?;
    let mut session = session.lock().map_err(|_| RuntimeError::Poisoned)?;
    operation(&mut session)
}

#[derive(Default)]
struct CompletedHistories {
    entries: HashMap<Key, SessionHistory>,
    order: VecDeque<Key>,
}

fn completed() -> &'static Mutex<CompletedHistories> {
    static COMPLETED: OnceLock<Mutex<CompletedHistories>> = OnceLock::new();
    COMPLETED.get_or_init(|| Mutex::new(CompletedHistories::default()))
}

pub(crate) fn remember_history(key: Key, history: SessionHistory) -> Result<(), RuntimeError> {
    let mut completed = completed().lock().map_err(|_| RuntimeError::Poisoned)?;
    completed.entries.insert(key.clone(), history);
    completed.order.retain(|candidate| candidate != &key);
    completed.order.push_back(key);
    while completed.order.len() > 64 {
        if let Some(expired) = completed.order.pop_front() {
            completed.entries.remove(&expired);
        }
    }
    Ok(())
}

pub(crate) fn completed_history(
    group_id: &str,
    actor_id: &str,
) -> Result<Option<SessionHistory>, RuntimeError> {
    completed()
        .lock()
        .map_err(|_| RuntimeError::Poisoned)
        .map(|completed| {
            completed
                .entries
                .get(&(group_id.to_owned(), actor_id.to_owned()))
                .cloned()
        })
}

pub(crate) fn discard_completed(key: &Key) -> Result<(), RuntimeError> {
    let mut completed = completed().lock().map_err(|_| RuntimeError::Poisoned)?;
    completed.entries.remove(key);
    completed.order.retain(|candidate| candidate != key);
    Ok(())
}

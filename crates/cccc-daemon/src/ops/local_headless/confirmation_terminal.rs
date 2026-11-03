//! A confirmation terminal remains isolated from task input after its watcher is cancelled.
use super::supervisor::Key;
use cccc_runtime::{RuntimeError, SessionStatus};
use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    pid: Option<u32>,
    started_at: String,
}

fn terminals() -> &'static Mutex<HashMap<Key, Identity>> {
    static TERMINALS: OnceLock<Mutex<HashMap<Key, Identity>>> = OnceLock::new();
    TERMINALS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn protect(status: &SessionStatus) -> io::Result<()> {
    terminals().lock().map_err(|_| super::poisoned())?.insert(
        (status.group_id.clone(), status.actor_id.clone()),
        Identity {
            pid: status.pid,
            started_at: status.started_at.clone(),
        },
    );
    Ok(())
}

pub(super) fn owns(group_id: &str, actor_id: &str) -> io::Result<bool> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    let identity = terminals()
        .lock()
        .map_err(|_| super::poisoned())?
        .get(&key)
        .cloned();
    let Some(identity) = identity else {
        return Ok(false);
    };
    let current = match cccc_runtime::status(group_id, actor_id) {
        Ok(status) => Some(status),
        Err(RuntimeError::NotFound(_, _)) => None,
        Err(error) => return Err(io::Error::other(error)),
    };
    if current.is_some_and(|status| {
        status.running && status.pid == identity.pid && status.started_at == identity.started_at
    }) {
        return Ok(true);
    }
    let mut entries = terminals().lock().map_err(|_| super::poisoned())?;
    match entries.get(&key) {
        Some(current) if *current == identity => {
            entries.remove(&key);
            Ok(false)
        }
        // Another confirmation replaced the observed entry while status was read.
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

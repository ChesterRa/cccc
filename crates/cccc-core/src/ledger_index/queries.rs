use cccc_contracts::Event;
use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;

use super::current;

pub(crate) fn inspect<T>(
    path: &Path,
    inspect: impl FnOnce(&[Event], &HashMap<String, usize>) -> T,
) -> io::Result<T> {
    let entry = current(path)?;
    let index = entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(inspect(&index.events, &index.positions))
}

pub(crate) fn inspect_status<T>(
    path: &Path,
    inspect: impl FnOnce(
        &[Event],
        &HashMap<String, usize>,
        &HashMap<String, BTreeSet<String>>,
        &HashMap<String, BTreeSet<String>>,
    ) -> T,
) -> io::Result<T> {
    let entry = current(path)?;
    let index = entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(inspect(
        &index.events,
        &index.positions,
        &index.acked_by,
        &index.replied_by,
    ))
}

pub(crate) fn find_event(path: &Path, event_id: &str) -> io::Result<Option<Event>> {
    let entry = current(path)?;
    let index = entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(index
        .positions
        .get(event_id)
        .and_then(|position| index.events.get(*position))
        .cloned())
}

pub(crate) fn find_idempotent(
    path: &Path,
    kind: &str,
    by: &str,
    client_id: &str,
) -> io::Result<Option<Event>> {
    let entry = current(path)?;
    let index = entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (kind.to_owned(), by.to_owned(), client_id.to_owned());
    Ok(index
        .client_ids
        .get(&key)
        .and_then(|position| index.events.get(*position))
        .cloned())
}

pub(crate) fn find_relay(path: &Path, source_event_id: &str) -> io::Result<Option<Event>> {
    let entry = current(path)?;
    let index = entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(index
        .relays
        .get(source_event_id)
        .and_then(|position| index.events.get(*position))
        .cloned())
}

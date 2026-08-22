use cccc_contracts::Event;
use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::ledger::{SourceRevision, read_all_uncached, revisions};

mod cache;
mod queries;
pub(crate) use queries::{find_event, find_idempotent, find_relay, inspect, inspect_status};

type ClientKey = (String, String, String);

#[derive(Default)]
struct LedgerIndex {
    revisions: Vec<SourceRevision>,
    events: Vec<Event>,
    positions: HashMap<String, usize>,
    client_ids: HashMap<ClientKey, usize>,
    relays: HashMap<String, usize>,
    replied_by: HashMap<String, BTreeSet<String>>,
    estimated_bytes: u64,
}

impl LedgerIndex {
    fn rebuild(path: &Path, revisions: Vec<SourceRevision>) -> io::Result<Self> {
        let events = read_all_uncached(path)?;
        let mut index = Self {
            revisions,
            events,
            ..Self::default()
        };
        index.reindex();
        index.estimated_bytes = estimate_events_bytes(&index.events);
        Ok(index)
    }

    fn reindex(&mut self) {
        self.positions.clear();
        self.client_ids.clear();
        self.relays.clear();
        self.replied_by.clear();
        for (position, event) in self.events.iter().enumerate() {
            self.positions.insert(event.id.clone(), position);
            if let Some(client_id) = event
                .data
                .get("client_id")
                .and_then(serde_json::Value::as_str)
            {
                self.client_ids.insert(
                    (event.kind.clone(), event.by.clone(), client_id.to_owned()),
                    position,
                );
            }
            if event.kind == "chat.message"
                && let Some(source_id) = event
                    .data
                    .get("src_event_id")
                    .and_then(serde_json::Value::as_str)
            {
                self.relays.insert(source_id.to_owned(), position);
            }
            index_reply(event, &mut self.replied_by);
        }
    }

    fn push(&mut self, event: Event, next_revisions: Vec<SourceRevision>) {
        let position = self.events.len();
        self.positions.insert(event.id.clone(), position);
        if let Some(client_id) = event
            .data
            .get("client_id")
            .and_then(serde_json::Value::as_str)
        {
            self.client_ids.insert(
                (event.kind.clone(), event.by.clone(), client_id.to_owned()),
                position,
            );
        }
        if event.kind == "chat.message"
            && let Some(source_id) = event
                .data
                .get("src_event_id")
                .and_then(serde_json::Value::as_str)
        {
            self.relays.insert(source_id.to_owned(), position);
        }
        index_reply(&event, &mut self.replied_by);
        self.estimated_bytes = self
            .estimated_bytes
            .saturating_add(estimate_event_bytes(&event));
        self.events.push(event);
        self.revisions = next_revisions;
    }
}

fn index_reply(event: &Event, replied_by: &mut HashMap<String, BTreeSet<String>>) {
    if event.kind != "chat.message" {
        return;
    }
    let target = event
        .data
        .get("reply_to")
        .and_then(serde_json::Value::as_str);
    let actor = Some(event.by.as_str());
    if let (Some(target), Some(actor)) = (target, actor)
        && !target.is_empty()
        && !actor.is_empty()
    {
        replied_by
            .entry(target.to_owned())
            .or_default()
            .insert(actor.to_owned());
    }
}

fn estimate_events_bytes(events: &[Event]) -> u64 {
    events.iter().map(estimate_event_bytes).sum()
}

fn estimate_event_bytes(event: &Event) -> u64 {
    let strings = [
        &event.id,
        &event.ts,
        &event.kind,
        &event.group_id,
        &event.scope_key,
        &event.by,
    ]
    .into_iter()
    .map(|value| value.capacity() as u64)
    .sum::<u64>();
    // IDs and relation keys are duplicated by the lookup maps. A factor of
    // two keeps the cache budget conservative without a second allocation.
    (std::mem::size_of::<Event>() as u64)
        .saturating_add(strings)
        .saturating_add(estimate_map_bytes(&event.data))
        .saturating_mul(2)
}

fn estimate_map_bytes(map: &serde_json::Map<String, serde_json::Value>) -> u64 {
    map.iter()
        .map(|(key, value)| key.capacity() as u64 + estimate_value_bytes(value))
        .sum::<u64>()
        .saturating_add((map.len() * std::mem::size_of::<(String, serde_json::Value)>()) as u64)
}

fn estimate_value_bytes(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::String(value) => value.capacity() as u64,
        serde_json::Value::Array(values) => values
            .iter()
            .map(estimate_value_bytes)
            .sum::<u64>()
            .saturating_add((values.capacity() * std::mem::size_of::<serde_json::Value>()) as u64),
        serde_json::Value::Object(map) => estimate_map_bytes(map),
        _ => std::mem::size_of::<serde_json::Value>() as u64,
    }
}

fn current(path: &Path) -> io::Result<Arc<RwLock<LedgerIndex>>> {
    let next_revisions = revisions(path)?;
    let weight = next_revisions.iter().map(|revision| revision.len).sum();
    let entry = cache::entry(path, weight);
    if entry
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .revisions
        == next_revisions
    {
        return Ok(entry);
    }
    let mut index = entry
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if index.revisions != next_revisions {
        *index = LedgerIndex::rebuild(path, next_revisions)?;
    }
    let weight = weight.max(index.estimated_bytes);
    drop(index);
    cache::update_weight(path, weight, &entry);
    Ok(entry)
}

pub(crate) fn note_append(path: &Path, event: &Event, encoded_len: usize) {
    let cached = cache::get(path);
    let Some(cached) = cached else { return };
    let Ok(next_revisions) = revisions(path) else {
        return;
    };
    let source_bytes: u64 = next_revisions.iter().map(|revision| revision.len).sum();
    let mut index = cached
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous_len = index
        .revisions
        .iter()
        .find(|revision| revision.path == path)
        .map(|revision| revision.len);
    let next_len = next_revisions
        .iter()
        .find(|revision| revision.path == path)
        .map(|revision| revision.len);
    let other_sources_unchanged = index
        .revisions
        .iter()
        .filter(|revision| revision.path != path)
        .eq(next_revisions
            .iter()
            .filter(|revision| revision.path != path));
    let exact_append = previous_len
        .zip(next_len)
        .is_some_and(|(before, after)| after == before.saturating_add(encoded_len as u64));
    if exact_append && other_sources_unchanged {
        index.push(event.clone(), next_revisions);
        let weight = source_bytes.max(index.estimated_bytes);
        drop(index);
        cache::update_weight(path, weight, &cached);
    } else {
        index.revisions.clear();
    }
}

pub(crate) fn invalidate_path(path: &Path) {
    cache::invalidate(path);
}

#[cfg(test)]
pub(crate) fn is_cached(path: &Path) -> bool {
    cache::get(path).is_some()
}

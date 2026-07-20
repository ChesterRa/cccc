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
    acked_by: HashMap<String, BTreeSet<String>>,
    replied_by: HashMap<String, BTreeSet<String>>,
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
        Ok(index)
    }

    fn reindex(&mut self) {
        self.positions.clear();
        self.client_ids.clear();
        self.relays.clear();
        self.acked_by.clear();
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
            index_relation(event, &mut self.acked_by, &mut self.replied_by);
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
        index_relation(&event, &mut self.acked_by, &mut self.replied_by);
        self.events.push(event);
        self.revisions = next_revisions;
    }
}

fn index_relation(
    event: &Event,
    acked_by: &mut HashMap<String, BTreeSet<String>>,
    replied_by: &mut HashMap<String, BTreeSet<String>>,
) {
    let (target, actor, relation) = if event.kind == "chat.ack" {
        (
            event
                .data
                .get("event_id")
                .and_then(serde_json::Value::as_str),
            event
                .data
                .get("actor_id")
                .and_then(serde_json::Value::as_str),
            acked_by,
        )
    } else if event.kind == "chat.message" {
        (
            event
                .data
                .get("reply_to")
                .and_then(serde_json::Value::as_str),
            Some(event.by.as_str()),
            replied_by,
        )
    } else {
        return;
    };
    if let (Some(target), Some(actor)) = (target, actor)
        && !target.is_empty()
        && !actor.is_empty()
    {
        relation
            .entry(target.to_owned())
            .or_default()
            .insert(actor.to_owned());
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
    drop(index);
    Ok(entry)
}

pub(crate) fn note_append(path: &Path, event: &Event, encoded_len: usize) {
    let cached = cache::get(path);
    let Some(cached) = cached else { return };
    let Ok(next_revisions) = revisions(path) else {
        return;
    };
    let weight = next_revisions.iter().map(|revision| revision.len).sum();
    cache::update_weight(path, weight, &cached);
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
    } else {
        index.revisions.clear();
    }
}

pub(crate) fn invalidate_path(path: &Path) {
    cache::invalidate(path);
}

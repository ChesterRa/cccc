use cccc_contracts::{ActorRole, Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::actors::effective_role;
use crate::fs::{read_json, write_json};
use crate::ledger;
use crate::{GroupDoc, GroupStore, HomeLayout};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InboxState {
    #[serde(default)]
    cursors: BTreeMap<String, String>,
}

pub fn list_unread(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    limit: usize,
) -> io::Result<Vec<Event>> {
    let mut unread = list_unread_many(home, group, &[actor_id.to_owned()], limit)?;
    Ok(unread.remove(actor_id).unwrap_or_default())
}

pub fn list_unread_many(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_ids: &[String],
    limit: usize,
) -> io::Result<BTreeMap<String, Vec<Event>>> {
    if actor_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let store = GroupStore::new(home.clone())?;
    let events = ledger::read_all(&store.ledger_path(&group.group_id)?)?;
    let state = load(home, &group.group_id)?;
    Ok(actor_ids
        .iter()
        .map(|actor_id| {
            let cursor = state.cursors.get(actor_id);
            let start = cursor
                .and_then(|id| events.iter().position(|event| &event.id == id))
                .map_or(0, |index| index + 1);
            let unread = events[start..]
                .iter()
                .filter(|event| is_for_actor(group, event, actor_id))
                .take(limit.min(1000))
                .cloned()
                .collect();
            (actor_id.clone(), unread)
        })
        .collect())
}

pub fn mark_read(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    event_id: &str,
) -> io::Result<()> {
    advance(home, group_id, actor_id, event_id).map(|_| ())
}

pub fn advance(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    event_id: &str,
) -> io::Result<bool> {
    let store = GroupStore::new(home.clone())?;
    let events = ledger::read_all(&store.ledger_path(group_id)?)?;
    let next = events
        .iter()
        .position(|event| event.id == event_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "event not found"))?;
    let mut state = load(home, group_id)?;
    let current = state
        .cursors
        .get(actor_id)
        .and_then(|current| events.iter().position(|event| &event.id == current));
    if current.is_some_and(|current| current >= next) {
        return Ok(false);
    }
    state.cursors.insert(actor_id.into(), event_id.into());
    write_json(&path(home, group_id)?, &state)?;
    Ok(true)
}

pub fn cursor(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<Option<String>> {
    Ok(load(home, group_id)?.cursors.get(actor_id).cloned())
}

pub fn is_for_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    if event.by == actor_id || !matches!(event.kind.as_str(), "chat.message" | "system.notify") {
        return false;
    }
    let to: Vec<_> = event
        .data
        .get("to")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default();
    if event.kind == "system.notify" && to.is_empty() {
        return event
            .data
            .get("actor_id")
            .and_then(|value| value.as_str())
            .is_none_or(|id| id == actor_id);
    }
    to.is_empty()
        || to.contains(&actor_id)
        || to.contains(&"@all")
        || (to.contains(&"@peers") && effective_role(group, actor_id) == Some(ActorRole::Peer))
        || (to.contains(&"@foreman") && effective_role(group, actor_id) == Some(ActorRole::Foreman))
}

fn load(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    let path = path(home, group_id)?;
    if path.exists() {
        read_json(&path)
    } else {
        Ok(InboxState::default())
    }
}

fn path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("inbox.json"))
}

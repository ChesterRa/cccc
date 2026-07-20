use cccc_contracts::{ActorRole, Event};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;

use crate::actors::{effective_role, find};
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
    let state = load(home, &group.group_id)?;
    let legacy = load_legacy(home, &group.group_id);
    ledger::inspect(&store.ledger_path(&group.group_id)?, |events, positions| {
        let state = merge_legacy(state, legacy, positions);
        actor_ids
            .iter()
            .map(|actor_id| {
                let start = state
                    .cursors
                    .get(actor_id)
                    .and_then(|id| positions.get(id))
                    .map_or(0, |index| index + 1);
                let unread = events[start..]
                    .iter()
                    .filter(|event| is_for_actor(group, event, actor_id))
                    .take(limit.min(1000))
                    .cloned()
                    .collect();
                (actor_id.clone(), unread)
            })
            .collect()
    })
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
    let state = load(home, group_id)?;
    let legacy = load_legacy(home, group_id);
    let (mut state, current, next) =
        ledger::inspect(&store.ledger_path(group_id)?, |_, positions| {
            let state = merge_legacy(state, legacy, positions);
            let next = positions.get(event_id).copied();
            let current = state
                .cursors
                .get(actor_id)
                .and_then(|current| positions.get(current))
                .copied();
            (state, current, next)
        })?;
    let next = next.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "event not found"))?;
    if current.is_some_and(|current| current >= next) {
        return Ok(false);
    }
    state.cursors.insert(actor_id.into(), event_id.into());
    write_json(&path(home, group_id)?, &state)?;
    Ok(true)
}

pub fn cursor(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<Option<String>> {
    Ok(load_effective(home, group_id)?
        .cursors
        .get(actor_id)
        .cloned())
}

pub fn cursors(home: &HomeLayout, group_id: &str) -> io::Result<BTreeMap<String, String>> {
    Ok(load_effective(home, group_id)?.cursors)
}

pub fn is_for_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    if event.by == actor_id || !matches!(event.kind.as_str(), "chat.message" | "system.notify") {
        return false;
    }
    if is_legacy_chat_notice(event) {
        return false;
    }
    let to: Vec<_> = event
        .data
        .get("to")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default();
    let internal = find(group, actor_id).is_some_and(|actor| actor.internal_kind.is_some());
    if event.kind == "system.notify" {
        let direct_target = ["target_actor_id", "actor_id"].iter().find_map(|key| {
            event
                .data
                .get(*key)
                .and_then(Value::as_str)
                .filter(|target| !target.is_empty())
        });
        if let Some(target) = direct_target {
            return target == actor_id;
        }
        if internal {
            return to.contains(&actor_id);
        }
        if to.is_empty() {
            return true;
        }
    } else if internal {
        return to.contains(&actor_id);
    }
    to.is_empty()
        || to.contains(&actor_id)
        || to.contains(&"@all")
        || (to.contains(&"@peers") && effective_role(group, actor_id) == Some(ActorRole::Peer))
        || (to.contains(&"@foreman") && effective_role(group, actor_id) == Some(ActorRole::Foreman))
}

fn is_legacy_chat_notice(event: &Event) -> bool {
    if event.kind != "system.notify" {
        return false;
    }
    let title = event
        .data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = event
        .data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let context = event.data.get("context").and_then(Value::as_object);
    matches!(
        title,
        "New message" | "Needs acknowledgement" | "Need reply"
    ) && message.starts_with("New message from ")
        && message.ends_with("Check your inbox.")
        && context
            .and_then(|value| value.get("event_id"))
            .and_then(Value::as_str)
            .is_some_and(|event_id| !event_id.is_empty())
}

fn load(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    let path = path(home, group_id)?;
    if path.exists() {
        read_json(&path)
    } else {
        Ok(InboxState::default())
    }
}

fn load_effective(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    let store = GroupStore::new(home.clone())?;
    let state = load(home, group_id)?;
    let legacy = load_legacy(home, group_id);
    ledger::inspect(&store.ledger_path(group_id)?, |_, positions| {
        merge_legacy(state, legacy, positions)
    })
}

fn load_legacy(home: &HomeLayout, group_id: &str) -> BTreeMap<String, String> {
    let Ok(path) = GroupStore::new(home.clone())
        .and_then(|store| store.state_dir(group_id))
        .map(|state| state.join("read_cursors.json"))
    else {
        return BTreeMap::new();
    };
    let Ok(doc) = read_json::<Value>(&path) else {
        return BTreeMap::new();
    };
    doc.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(actor_id, cursor)| {
            cursor
                .as_str()
                .or_else(|| cursor.get("event_id").and_then(Value::as_str))
                .filter(|event_id| !event_id.is_empty())
                .map(|event_id| (actor_id.clone(), event_id.to_owned()))
        })
        .collect()
}

fn merge_legacy(
    mut state: InboxState,
    legacy: BTreeMap<String, String>,
    positions: &HashMap<String, usize>,
) -> InboxState {
    for (actor_id, legacy_id) in legacy {
        let Some(legacy_position) = positions.get(&legacy_id) else {
            continue;
        };
        let current_position = state
            .cursors
            .get(&actor_id)
            .and_then(|event_id| positions.get(event_id));
        if current_position.is_none_or(|current| legacy_position > current) {
            state.cursors.insert(actor_id, legacy_id);
        }
    }
    state
}

fn path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("inbox.json"))
}

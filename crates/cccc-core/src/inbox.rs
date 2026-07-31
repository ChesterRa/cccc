use cccc_contracts::{ActorRole, Event};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::actors::{effective_role, find};
use crate::fs::{read_json, write_json};
use crate::ledger;
use crate::{GroupDoc, GroupStore, HomeLayout};

#[derive(Debug, Clone, Default)]
struct InboxState {
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
    ledger::inspect(&store.ledger_path(&group.group_id)?, |events, positions| {
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
    let (mut state, current, next, next_ts) =
        ledger::inspect(&store.ledger_path(group_id)?, |events, positions| {
            let next = positions.get(event_id).copied();
            let current = state
                .cursors
                .get(actor_id)
                .and_then(|current| positions.get(current))
                .copied();
            let next_ts = next
                .and_then(|index| events.get(index))
                .map(|event| event.ts.clone())
                .unwrap_or_default();
            (state, current, next, next_ts)
        })?;
    let next = next.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "event not found"))?;
    if current.is_some_and(|current| current >= next) {
        return Ok(false);
    }
    state.cursors.insert(actor_id.into(), event_id.into());
    save(home, group_id, &state, actor_id, &next_ts)?;
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
    if !path.exists() {
        migrate_legacy_inbox(home, group_id, &path)?;
    }
    let doc = if path.exists() {
        read_json::<Value>(&path)?
    } else {
        Value::Object(Map::new())
    };
    let cursors = doc
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(actor_id, cursor)| {
            cursor
                .as_str()
                .or_else(|| cursor.get("event_id").and_then(Value::as_str))
                .filter(|event_id| !event_id.is_empty())
                .map(|event_id| (actor_id.clone(), event_id.to_owned()))
        })
        .collect();
    Ok(InboxState { cursors })
}

fn save(
    home: &HomeLayout,
    group_id: &str,
    state: &InboxState,
    updated_actor_id: &str,
    updated_ts: &str,
) -> io::Result<()> {
    let path = path(home, group_id)?;
    let previous = read_json::<Value>(&path).unwrap_or_else(|_| json!({}));
    let mut output = Map::new();
    for (actor_id, event_id) in &state.cursors {
        let ts = if actor_id == updated_actor_id {
            updated_ts
        } else {
            previous
                .get(actor_id)
                .and_then(|cursor| cursor.get("ts"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        output.insert(
            actor_id.clone(),
            json!({
                "event_id":event_id,
                "ts":ts,
                "updated_at":if actor_id == updated_actor_id {
                    cccc_contracts::utc_now()
                } else {
                    previous
                        .get(actor_id)
                        .and_then(|cursor| cursor.get("updated_at"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                },
            }),
        );
    }
    write_json(&path, &Value::Object(output))
}

fn migrate_legacy_inbox(
    home: &HomeLayout,
    group_id: &str,
    canonical: &std::path::Path,
) -> io::Result<()> {
    let legacy = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("inbox.json");
    if !legacy.exists() {
        return Ok(());
    }
    let doc = read_json::<Value>(&legacy)?;
    let cursors = doc
        .get("cursors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|(actor_id, event_id)| {
            (
                actor_id,
                json!({
                    "event_id":event_id.as_str().unwrap_or_default(),
                    "ts":"",
                    "updated_at":cccc_contracts::utc_now(),
                }),
            )
        })
        .collect::<Map<_, _>>();
    write_json(canonical, &Value::Object(cursors))
}

fn load_effective(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    load(home, group_id)
}

fn path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("read_cursors.json"))
}

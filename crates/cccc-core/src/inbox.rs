use cccc_contracts::{ActorRole, Event};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::actors::{effective_role, find};
use crate::fs::{read_json, with_exclusive_lock, write_json};
use crate::ledger;
use crate::{GroupDoc, GroupStore, HomeLayout};

const MAIL_CURSOR_SCHEMA: u64 = 1;
const PENDING_READ_SCHEMA: u64 = 1;

#[derive(Debug, Clone, Default)]
struct InboxState {
    cursors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct PendingCursor {
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    ts: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PendingRead {
    schema: u64,
    group_id: String,
    actor_id: String,
    expected: PendingCursor,
    target: PendingCursor,
}

#[derive(Debug, Clone)]
pub struct ConsumedInbox {
    pub messages: Vec<Event>,
    pub cursor_event_id: String,
    pub cursor_ts: String,
    pub cursor_updated_at: String,
    pub read_event: Option<Event>,
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
        let generations = actor_generation_positions(events);
        actor_ids
            .iter()
            .map(|actor_id| {
                let cursor_start = state
                    .cursors
                    .get(actor_id)
                    .and_then(|id| positions.get(id))
                    .map_or(0, |index| index + 1);
                let generation_start = generations.get(actor_id).copied().unwrap_or(0);
                let unread = events[cursor_start.max(generation_start)..]
                    .iter()
                    .filter(|event| is_mail_for_actor(group, event, actor_id))
                    .take(limit.min(1000))
                    .cloned()
                    .collect();
                (actor_id.clone(), unread)
            })
            .collect()
    })
}

pub fn consume_unread(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    by: &str,
    limit: usize,
) -> io::Result<ConsumedInbox> {
    let store = GroupStore::new(home.clone())?;
    let cursor_path = path(home, &group.group_id)?;
    let lock_path = cursor_path.with_file_name("read_cursors.json.lock");
    with_exclusive_lock(&lock_path, || {
        recover_pending_locked(home, &group.group_id)?;
        let mut state = load_raw(home, &group.group_id)?;
        let messages =
            ledger::inspect(&store.ledger_path(&group.group_id)?, |events, positions| {
                let cursor_start = state
                    .cursors
                    .get(actor_id)
                    .and_then(|id| positions.get(id))
                    .map_or(0, |index| index + 1);
                let generation_start = actor_generation_positions(events)
                    .get(actor_id)
                    .copied()
                    .unwrap_or(0);
                events[cursor_start.max(generation_start)..]
                    .iter()
                    .filter(|event| is_mail_for_actor(group, event, actor_id))
                    .take(limit.min(200))
                    .cloned()
                    .collect::<Vec<_>>()
            })?;

        let Some(boundary) = messages.last() else {
            let (cursor_event_id, cursor_ts, cursor_updated_at) =
                cursor_details(home, &group.group_id, actor_id)?;
            return Ok(ConsumedInbox {
                messages,
                cursor_event_id,
                cursor_ts,
                cursor_updated_at,
                read_event: None,
            });
        };

        let mut read_event = Event::new("mail.read", &group.group_id);
        read_event.by = by.to_owned();
        read_event.data = json!({"actor_id":actor_id,"event_id":boundary.id})
            .as_object()
            .cloned()
            .expect("mail.read data is an object");
        let pending = PendingRead {
            schema: PENDING_READ_SCHEMA,
            group_id: group.group_id.clone(),
            actor_id: actor_id.to_owned(),
            expected: stored_cursor_record(home, &group.group_id, actor_id)?,
            target: PendingCursor {
                event_id: boundary.id.clone(),
                ts: boundary.ts.clone(),
                updated_at: cccc_contracts::utc_now(),
            },
        };
        write_json(&pending_path(home, &group.group_id)?, &pending)?;
        if let Err(append_error) = ledger::append(&store.ledger_path(&group.group_id)?, &read_event)
        {
            return match clear_pending(home, &group.group_id) {
                Ok(()) => Err(append_error),
                Err(cleanup_error) => Err(io::Error::other(format!(
                    "{append_error}; pending Mail read cleanup failed: {cleanup_error}"
                ))),
            };
        }
        state
            .cursors
            .insert(actor_id.to_owned(), boundary.id.clone());
        save(home, &group.group_id, &state, actor_id, &boundary.ts)?;
        let _ = clear_pending(home, &group.group_id);

        let (cursor_event_id, cursor_ts, cursor_updated_at) =
            cursor_details(home, &group.group_id, actor_id)?;
        Ok(ConsumedInbox {
            messages,
            cursor_event_id,
            cursor_ts,
            cursor_updated_at,
            read_event: Some(read_event),
        })
    })
}

pub fn mail_pending_summary(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
) -> io::Result<Option<Value>> {
    let store = GroupStore::new(home.clone())?;
    let state = load(home, &group.group_id)?;
    ledger::inspect(&store.ledger_path(&group.group_id)?, |events, positions| {
        let cursor_start = state
            .cursors
            .get(actor_id)
            .and_then(|event_id| positions.get(event_id))
            .map_or(0, |position| position + 1);
        let generation_start = actor_generation_positions(events)
            .get(actor_id)
            .copied()
            .unwrap_or(0);
        let start = cursor_start.max(generation_start);

        // Reply and manual-delivery facts suppress the one-shot active notice,
        // but they do not consume Mail. Natural hints must therefore mirror
        // the unread Inbox projection rather than the notice-eligible subset.
        let pending = events[start..]
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && event.by != actor_id
                    && event.data.get("message_mode").and_then(Value::as_str) == Some("mail")
                    && is_for_actor(group, event, actor_id)
            })
            .collect::<Vec<_>>();
        let oldest = pending.first()?;
        let oldest_age_seconds = DateTime::parse_from_rfc3339(&oldest.ts)
            .map(|created| {
                Utc::now()
                    .signed_duration_since(created.with_timezone(&Utc))
                    .num_seconds()
                    .max(0)
            })
            .unwrap_or(0);
        Some(json!({
            "count":pending.len(),
            "oldest_age_seconds":oldest_age_seconds,
            "action":"cccc_inbox_read()",
        }))
    })
}

pub fn cursor(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<Option<String>> {
    Ok(load_effective(home, group_id)?
        .cursors
        .get(actor_id)
        .cloned())
}

pub fn cursor_details(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<(String, String, String)> {
    let event_id = load_effective(home, group_id)?
        .cursors
        .get(actor_id)
        .cloned()
        .unwrap_or_default();
    let stored = stored_cursor_record(home, group_id, actor_id)?;
    if stored.event_id == event_id {
        return Ok((event_id, stored.ts, stored.updated_at));
    }
    if let Some(pending) = load_pending(home, group_id)?
        && pending.actor_id == actor_id
        && pending.target.event_id == event_id
        && pending_has_fact(home, group_id, &pending)?
    {
        return Ok((event_id, pending.target.ts, pending.target.updated_at));
    }
    Ok((event_id, String::new(), String::new()))
}

pub fn cursors(home: &HomeLayout, group_id: &str) -> io::Result<BTreeMap<String, String>> {
    Ok(load_effective(home, group_id)?.cursors)
}

pub fn is_mail_for_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    event.kind == "chat.message"
        && event.data.get("message_mode").and_then(Value::as_str) == Some("mail")
        && is_for_actor(group, event, actor_id)
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
    let internal = find(group, actor_id).is_some_and(|actor| actor.internal_kind.is_some());
    if event.kind == "system.notify" {
        if matches!(
            event.data.get("kind").and_then(Value::as_str),
            Some("mail_notice" | "reply_notice")
        ) {
            return false;
        }
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

pub fn actor_generation_positions(events: &[Event]) -> HashMap<String, usize> {
    let mut positions = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        if event.kind != "actor.add" {
            continue;
        }
        let actor_id = event
            .data
            .get("actor")
            .and_then(Value::as_object)
            .and_then(|actor| actor.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if !actor_id.is_empty() {
            positions.insert(actor_id.to_owned(), index);
        }
    }
    positions
}

pub fn actor_generation_contains(
    generations: &HashMap<String, usize>,
    event_positions: &HashMap<String, usize>,
    actor_id: &str,
    event: &Event,
) -> Option<bool> {
    let generation = generations.get(actor_id)?;
    let event_position = event_positions.get(&event.id)?;
    Some(event_position >= generation)
}

fn load_raw(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    let path = path(home, group_id)?;
    let doc = if path.exists() {
        read_json::<Value>(&path)?
    } else {
        Value::Object(Map::new())
    };
    let object = doc
        .as_object()
        .ok_or_else(|| io::Error::other("read cursor document must be an object"))?;
    if object.get("schema").and_then(Value::as_u64) != Some(MAIL_CURSOR_SCHEMA) {
        return Ok(InboxState::default());
    }
    let cursors = object
        .get("cursors")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("Mail cursor document is missing cursors"))?
        .iter()
        .filter_map(|(actor_id, cursor)| {
            cursor
                .get("event_id")
                .and_then(Value::as_str)
                .filter(|event_id| !event_id.is_empty())
                .map(|event_id| (actor_id.clone(), event_id.to_owned()))
        })
        .collect();
    Ok(InboxState { cursors })
}

fn load(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    let mut state = load_raw(home, group_id)?;
    if let Some(pending) = load_pending(home, group_id)?
        && pending_has_fact(home, group_id, &pending)?
    {
        let current = state
            .cursors
            .get(&pending.actor_id)
            .map(String::as_str)
            .unwrap_or_default();
        if !cursor_covers(home, group_id, current, &pending.target.event_id)? {
            state
                .cursors
                .insert(pending.actor_id, pending.target.event_id);
        }
    }
    Ok(state)
}

fn load_pending(home: &HomeLayout, group_id: &str) -> io::Result<Option<PendingRead>> {
    let path = pending_path(home, group_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let pending = read_json::<PendingRead>(&path)?;
    if pending.schema != PENDING_READ_SCHEMA
        || pending.group_id != group_id
        || pending.actor_id.trim().is_empty()
        || pending.target.event_id.trim().is_empty()
    {
        return Err(io::Error::other("pending Mail read document is invalid"));
    }
    Ok(Some(pending))
}

fn pending_has_fact(home: &HomeLayout, group_id: &str, pending: &PendingRead) -> io::Result<bool> {
    let ledger_path = GroupStore::new(home.clone())?.ledger_path(group_id)?;
    ledger::inspect(&ledger_path, |events, _| {
        events.iter().rev().any(|event| {
            event.kind == "mail.read"
                && event.data.get("actor_id").and_then(Value::as_str)
                    == Some(pending.actor_id.as_str())
                && event.data.get("event_id").and_then(Value::as_str)
                    == Some(pending.target.event_id.as_str())
        })
    })
}

fn cursor_covers(
    home: &HomeLayout,
    group_id: &str,
    current_event_id: &str,
    target_event_id: &str,
) -> io::Result<bool> {
    if current_event_id == target_event_id {
        return Ok(true);
    }
    if current_event_id.is_empty() {
        return Ok(false);
    }
    let ledger_path = GroupStore::new(home.clone())?.ledger_path(group_id)?;
    ledger::inspect(&ledger_path, |_, positions| {
        positions
            .get(current_event_id)
            .zip(positions.get(target_event_id))
            .is_some_and(|(current, target)| current >= target)
    })
}

fn stored_cursor_record(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<PendingCursor> {
    let cursor_path = path(home, group_id)?;
    if !cursor_path.exists() {
        return Ok(PendingCursor::default());
    }
    let doc = read_json::<Value>(&cursor_path)?;
    let record = doc
        .get("cursors")
        .and_then(Value::as_object)
        .and_then(|cursors| cursors.get(actor_id))
        .and_then(Value::as_object);
    Ok(PendingCursor {
        event_id: record
            .and_then(|value| value.get("event_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        ts: record
            .and_then(|value| value.get("ts"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        updated_at: record
            .and_then(|value| value.get("updated_at"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn recover_pending_locked(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    let Some(pending) = load_pending(home, group_id)? else {
        return Ok(());
    };
    if !pending_has_fact(home, group_id, &pending)? {
        return clear_pending(home, group_id);
    }
    let mut state = load_raw(home, group_id)?;
    let current = stored_cursor_record(home, group_id, &pending.actor_id)?;
    if cursor_covers(home, group_id, &current.event_id, &pending.target.event_id)? {
        return clear_pending(home, group_id);
    }
    if current.event_id != pending.expected.event_id || current.ts != pending.expected.ts {
        return Err(io::Error::other(
            "pending Mail read cursor changed concurrently",
        ));
    }
    state
        .cursors
        .insert(pending.actor_id.clone(), pending.target.event_id.clone());
    save(
        home,
        group_id,
        &state,
        &pending.actor_id,
        &pending.target.ts,
    )?;
    clear_pending(home, group_id)
}

fn clear_pending(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    match fs::remove_file(pending_path(home, group_id)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn save(
    home: &HomeLayout,
    group_id: &str,
    state: &InboxState,
    updated_actor_id: &str,
    updated_ts: &str,
) -> io::Result<()> {
    let path = path(home, group_id)?;
    let previous = if path.exists() {
        read_json::<Value>(&path)?
            .as_object()
            .cloned()
            .ok_or_else(|| io::Error::other("read cursor document must be an object"))?
            .get("cursors")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    } else {
        Map::new()
    };
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
    write_json(
        &path,
        &json!({"schema":MAIL_CURSOR_SCHEMA,"cursors":output}),
    )
}

fn load_effective(home: &HomeLayout, group_id: &str) -> io::Result<InboxState> {
    load(home, group_id)
}

fn path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("read_cursors.json"))
}

fn pending_path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(path(home, group_id)?.with_file_name("read_cursors.pending.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_consumers_return_each_message_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("inbox consume", "").expect("group");
        group.actors.push(Actor::new("peer1"));
        store.save(&group).expect("save group");

        let mut message = Event::new("chat.message", &group.group_id);
        message.by = "user".into();
        message.data = json!({
            "text":"only once",
            "to":["peer1"],
            "message_mode":"mail",
        })
        .as_object()
        .cloned()
        .expect("message data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &message,
        )
        .expect("append message");

        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let home = home.clone();
            let group = group.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                consume_unread(&home, &group, "peer1", "peer1", 50).expect("consume")
            }));
        }
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("consumer"))
            .collect::<Vec<_>>();

        assert_eq!(
            results
                .iter()
                .map(|result| result.messages.len())
                .sum::<usize>(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result.read_event.is_some())
                .count(),
            1
        );
        let read_count = ledger::inspect(
            &store.ledger_path(&group.group_id).expect("ledger"),
            |events, _| {
                events
                    .iter()
                    .filter(|event| event.kind == "mail.read")
                    .count()
            },
        )
        .expect("read ledger");
        assert_eq!(read_count, 1);
    }

    #[test]
    fn ledger_committed_pending_read_recovers_without_replaying_mail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("inbox recovery", "").expect("group");
        group.actors.push(Actor::new("peer1"));
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");

        let mut message = Event::new("chat.message", &group.group_id);
        message.by = "user".into();
        message.data = json!({
            "text":"commit once",
            "to":["peer1"],
            "message_mode":"mail",
        })
        .as_object()
        .cloned()
        .expect("message data");
        ledger::append(&ledger_path, &message).expect("append message");

        let mut read_event = Event::new("mail.read", &group.group_id);
        read_event.by = "peer1".into();
        read_event.data = json!({"actor_id":"peer1","event_id":message.id})
            .as_object()
            .cloned()
            .expect("read data");
        ledger::append(&ledger_path, &read_event).expect("append read");
        let pending = PendingRead {
            schema: PENDING_READ_SCHEMA,
            group_id: group.group_id.clone(),
            actor_id: "peer1".into(),
            expected: PendingCursor::default(),
            target: PendingCursor {
                event_id: message.id.clone(),
                ts: message.ts.clone(),
                updated_at: "2026-08-22T00:00:00Z".into(),
            },
        };
        write_json(
            &pending_path(&home, &group.group_id).expect("pending path"),
            &pending,
        )
        .expect("pending marker");

        assert!(
            list_unread(&home, &group, "peer1", 10)
                .expect("effective unread")
                .is_empty()
        );
        assert_eq!(
            cursor_details(&home, &group.group_id, "peer1").expect("effective cursor"),
            (
                message.id.clone(),
                message.ts.clone(),
                "2026-08-22T00:00:00Z".into()
            )
        );

        let recovered =
            consume_unread(&home, &group, "peer1", "peer1", 10).expect("recover pending read");
        assert!(recovered.messages.is_empty());
        assert!(recovered.read_event.is_none());
        assert_eq!(recovered.cursor_event_id, message.id);
        assert!(
            !pending_path(&home, &group.group_id)
                .expect("pending path")
                .exists()
        );
        let read_count = ledger::inspect(&ledger_path, |events, _| {
            events
                .iter()
                .filter(|event| event.kind == "mail.read")
                .count()
        })
        .expect("read ledger");
        assert_eq!(read_count, 1);
    }

    #[test]
    fn natural_pending_summary_tracks_unread_mail_after_reply_and_push() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("pending summary", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");

        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.data = json!({
            "text":"read this later",
            "to":["peer1"],
            "message_mode":"mail",
        })
        .as_object()
        .cloned()
        .expect("mail data");
        ledger::append(&ledger_path, &mail).expect("append mail");

        let mut reply = Event::new("chat.message", &group.group_id);
        reply.by = actor.id.clone();
        reply.data = json!({
            "text":"I handled the urgent part",
            "to":["user"],
            "reply_to":mail.id.as_str(),
            "message_mode":"send",
        })
        .as_object()
        .cloned()
        .expect("reply data");
        ledger::append(&ledger_path, &reply).expect("append reply");

        let mut delivery = Event::new("runtime.delivery", &group.group_id);
        delivery.by = "system".into();
        delivery.data = json!({
            "actor_id":"peer1",
            "source_event_id":mail.id.as_str(),
            "delivery_id":"delivery-1",
            "state":"accepted",
            "transport":"test",
        })
        .as_object()
        .cloned()
        .expect("delivery data");
        ledger::append(&ledger_path, &delivery).expect("append delivery");

        let pending = mail_pending_summary(&home, &group, "peer1")
            .expect("pending summary")
            .expect("one unread Mail");
        assert_eq!(pending["count"], 1);

        let consumed = consume_unread(&home, &group, "peer1", "peer1", 10).expect("consume");
        assert_eq!(consumed.messages.len(), 1);
        assert!(
            mail_pending_summary(&home, &group, "peer1")
                .expect("empty summary")
                .is_none()
        );
    }
}

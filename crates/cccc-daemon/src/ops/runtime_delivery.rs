use cccc_contracts::{Actor, Event};
use cccc_core::fs::with_exclusive_lock;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dispatch::OpError;

mod legacy_read_watermark;

use legacy_read_watermark::LegacyReadWatermark;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    Claimed,
    Terminal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome<'a> {
    Claimed,
    Accepted,
    Failed(&'a str),
    Ambiguous(&'a str),
}

impl<'a> DeliveryOutcome<'a> {
    fn parts(self) -> (&'static str, &'a str) {
        match self {
            Self::Claimed => ("claimed", ""),
            Self::Accepted => ("accepted", ""),
            Self::Failed(reason) => ("failed", reason),
            Self::Ambiguous(reason) => ("ambiguous", reason),
        }
    }
}

pub fn delivery_id(
    group_id: &str,
    actor_id: &str,
    actor_created_at: &str,
    source_event_id: &str,
) -> String {
    let seed = [group_id, actor_id, actor_created_at, source_event_id].join("\0");
    let digest = format!("{:x}", Sha256::digest(seed.as_bytes()));
    format!("delivery:{actor_id}:{}", &digest[..24])
}

pub fn append_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    actor_created_at: &str,
    source_event_id: &str,
    transport: &str,
    outcome: DeliveryOutcome<'_>,
) -> Result<Event, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let (state, reason) = outcome.parts();
    let mut event = Event::new("runtime.delivery", group_id);
    event.by = "system".into();
    event.data = json!({
        "actor_id":actor_id,
        "source_event_id":source_event_id,
        "delivery_id":delivery_id(group_id, actor_id, actor_created_at, source_event_id),
        "state":state,
        "transport":transport,
        "reason":if reason.is_empty() { Value::Null } else { Value::String(reason.into()) },
    })
    .as_object()
    .cloned()
    .expect("runtime delivery data");
    ledger::append(&store.ledger_path(group_id).map_err(OpError::io)?, &event)
        .map_err(OpError::io)?;
    Ok(event)
}

pub fn latest_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    source_event_id: &str,
) -> Result<Option<(String, String)>, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let events = ledger::read_all(&store.ledger_path(group_id).map_err(OpError::io)?)
        .map_err(OpError::io)?;
    Ok(events.iter().rev().find_map(|event| {
        (event.kind == "runtime.delivery"
            && event.data.get("actor_id").and_then(Value::as_str) == Some(actor_id)
            && event.data.get("source_event_id").and_then(Value::as_str) == Some(source_event_id))
        .then(|| {
            (
                event
                    .data
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                event
                    .data
                    .get("transport")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
    }))
}

pub fn claim(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    source_event_id: &str,
    transport: &str,
    force_ambiguous: bool,
) -> Result<ClaimResult, OpError> {
    let deliveries = [(actor, transport)];
    let (claimed, states) =
        claim_deliveries(home, group, &deliveries, source_event_id, force_ambiguous)?;
    if claimed {
        Ok(ClaimResult::Claimed)
    } else {
        Ok(ClaimResult::Terminal(
            states.get(&actor.id).cloned().unwrap_or_default(),
        ))
    }
}

pub fn claim_deliveries(
    home: &HomeLayout,
    group: &GroupDoc,
    deliveries: &[(&Actor, &str)],
    source_event_id: &str,
    force_ambiguous: bool,
) -> Result<(bool, HashMap<String, String>), OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let lock_path = delivery_lock_path(&ledger_path);
    with_exclusive_lock(&lock_path, || {
        let events = ledger::read_all(&ledger_path)?;
        let mut states = HashMap::new();
        for (actor, _transport) in deliveries {
            let state = events
                .iter()
                .rev()
                .find_map(|event| {
                    (event.kind == "runtime.delivery"
                        && event.data.get("actor_id").and_then(Value::as_str)
                            == Some(actor.id.as_str())
                        && event.data.get("source_event_id").and_then(Value::as_str)
                            == Some(source_event_id))
                    .then(|| {
                        event
                            .data
                            .get("state")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    })
                })
                .unwrap_or_default();
            states.insert(actor.id.clone(), state.clone());
            if state == "claimed" {
                return Ok((false, states));
            }
            if state == "accepted" || (state == "ambiguous" && !force_ambiguous) {
                return Ok((false, states));
            }
        }
        for (actor, transport) in deliveries {
            append_state(
                home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                source_event_id,
                transport,
                DeliveryOutcome::Claimed,
            )
            .map_err(|error| std::io::Error::other(error.message))?;
            states.insert(actor.id.clone(), "claimed".into());
        }
        Ok((true, states))
    })
    .map_err(OpError::io)
}

pub fn settle_stranded_claims(home: &HomeLayout, group: &GroupDoc) -> Result<usize, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    with_exclusive_lock(&delivery_lock_path(&ledger_path), || {
        let events = ledger::read_all(&ledger_path)?;
        let mut latest = HashMap::<(String, String), (String, String)>::new();
        for event in events {
            if event.kind == "actor.add" {
                let actor_id = event
                    .data
                    .get("actor")
                    .and_then(Value::as_object)
                    .and_then(|actor| actor.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !actor_id.is_empty() {
                    latest.retain(|(existing, _), _| existing != actor_id);
                }
                continue;
            }
            if event.kind != "runtime.delivery" {
                continue;
            }
            let actor_id = event
                .data
                .get("actor_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let source_event_id = event
                .data
                .get("source_event_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if actor_id.is_empty() || source_event_id.is_empty() {
                continue;
            }
            latest.insert(
                (actor_id.into(), source_event_id.into()),
                (
                    event
                        .data
                        .get("state")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    event
                        .data
                        .get("transport")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                ),
            );
        }
        let mut settled = 0;
        for ((actor_id, source_event_id), (state, transport)) in latest {
            let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
                continue;
            };
            if state != "claimed" {
                continue;
            }
            append_state(
                home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                &source_event_id,
                &transport,
                DeliveryOutcome::Ambiguous(
                    "daemon restarted before the claimed handoff recorded an outcome",
                ),
            )
            .map_err(|error| std::io::Error::other(error.message))?;
            settled += 1;
        }
        Ok(settled)
    })
    .map_err(OpError::io)
}

fn delivery_lock_path(ledger_path: &Path) -> PathBuf {
    ledger_path
        .parent()
        .expect("group ledger has a parent")
        .join("state/ledger/runtime_delivery.lock")
}

pub fn pending_sources(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    limit: usize,
) -> Result<Vec<Event>, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let events = ledger::read_all(&store.ledger_path(&group.group_id).map_err(OpError::io)?)
        .map_err(OpError::io)?;
    let generation = events
        .iter()
        .rposition(|event| {
            event.kind == "actor.add"
                && event
                    .data
                    .get("actor")
                    .and_then(Value::as_object)
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    == Some(actor.id.as_str())
        })
        .map(|index| index + 1)
        .unwrap_or(0);
    let generation_events = &events[generation..];
    let legacy_read_watermark = LegacyReadWatermark::from_events(generation_events, &actor.id);
    let mut latest = Map::<String, Value>::new();
    for event in generation_events {
        if event.kind != "runtime.delivery"
            || event.data.get("actor_id").and_then(Value::as_str) != Some(actor.id.as_str())
        {
            continue;
        }
        if let (Some(source), Some(state)) = (
            event.data.get("source_event_id").and_then(Value::as_str),
            event.data.get("state").and_then(Value::as_str),
        ) {
            latest.insert(source.to_owned(), Value::String(state.to_owned()));
        }
    }
    let mut pending = Vec::new();
    for event in generation_events {
        if event.by == actor.id {
            continue;
        }
        let state = latest
            .get(&event.id)
            .and_then(Value::as_str)
            .unwrap_or_default();
        let addressed = if event.kind == "chat.message" {
            let mode = event
                .data
                .get("message_mode")
                .and_then(Value::as_str)
                .unwrap_or_default();
            (matches!(mode, "send" | "request_reply") || (mode == "mail" && state == "claimed"))
                && inbox::is_for_actor(group, event, &actor.id)
        } else if event.kind == "system.notify" {
            if legacy_read_watermark.covers_notification(event) {
                continue;
            }
            let notice_kind = event
                .data
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(notice_kind, "mail_notice" | "reply_notice") {
                event.data.get("target_actor_id").and_then(Value::as_str) == Some(actor.id.as_str())
            } else {
                inbox::is_for_actor(group, event, &actor.id)
            }
        } else {
            false
        };
        if !addressed {
            continue;
        }
        if !matches!(state, "accepted" | "ambiguous") {
            pending.push(event.clone());
            if pending.len() >= limit.max(1) {
                break;
            }
        }
    }
    Ok(pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_claims_are_unique_and_restart_settles_the_stranded_claim() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("runtime delivery", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");

        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let barrier = barrier.clone();
                let home = &home;
                let group = &group;
                let actor = &actor;
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    claim(home, group, actor, "source-1", "pty", false).expect("claim")
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().expect("claim thread"))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == ClaimResult::Claimed)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == ClaimResult::Terminal("claimed".into()))
                .count(),
            1
        );

        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        let states = ledger::read_all(&ledger_path)
            .expect("ledger")
            .into_iter()
            .filter(|event| event.kind == "runtime.delivery")
            .map(|event| {
                event
                    .data
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(states, ["claimed"]);

        assert_eq!(settle_stranded_claims(&home, &group).expect("settle"), 1);
        assert_eq!(
            latest_state(&home, &group.group_id, &actor.id, "source-1")
                .expect("latest")
                .expect("state")
                .0,
            "ambiguous"
        );
        assert_eq!(
            claim(&home, &group, &actor, "source-1", "pty", false).expect("blocked retry"),
            ClaimResult::Terminal("ambiguous".into())
        );
        assert_eq!(
            claim(&home, &group, &actor, "source-1", "pty", true).expect("forced retry"),
            ClaimResult::Claimed
        );
    }

    #[test]
    fn pending_sources_apply_legacy_read_watermark_to_notification_prefix() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("legacy read recovery", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");

        let mut old_message = Event::new("chat.message", &group.group_id);
        old_message.by = "user".into();
        old_message.data = json!({"to":["peer1"],"text":"already read"})
            .as_object()
            .cloned()
            .expect("message data");
        ledger::append(&ledger_path, &old_message).expect("old message");
        let mut first_notice = Event::new("system.notify", &group.group_id);
        first_notice.by = "system".into();
        first_notice.data = json!({
            "to":["peer1"],
            "kind":"info",
            "context":{"event_id":old_message.id}
        })
        .as_object()
        .cloned()
        .expect("notice data");
        ledger::append(&ledger_path, &first_notice).expect("first notice");
        let mut watermark_notice = Event::new("system.notify", &group.group_id);
        watermark_notice.by = "system".into();
        watermark_notice.data = json!({"to":["peer1"],"kind":"unread_nudge"})
            .as_object()
            .cloned()
            .expect("notice data");
        ledger::append(&ledger_path, &watermark_notice).expect("watermark notice");
        let mut read = Event::new("chat.read", &group.group_id);
        read.by = actor.id.clone();
        read.data = json!({"actor_id":actor.id,"event_id":watermark_notice.id})
            .as_object()
            .cloned()
            .expect("read data");
        ledger::append(&ledger_path, &read).expect("legacy read watermark");
        let mut late_linked_notice = Event::new("system.notify", &group.group_id);
        late_linked_notice.by = "system".into();
        late_linked_notice.data = json!({
            "to":["peer1"],
            "kind":"unread_nudge",
            "context":{"event_id":old_message.id}
        })
        .as_object()
        .cloned()
        .expect("late linked notice data");
        ledger::append(&ledger_path, &late_linked_notice).expect("late linked notice");
        let mut current_notice = Event::new("system.notify", &group.group_id);
        current_notice.by = "system".into();
        current_notice.data = json!({"to":["peer1"],"kind":"info"})
            .as_object()
            .cloned()
            .expect("current notice data");
        ledger::append(&ledger_path, &current_notice).expect("current notice");
        let mut current = Event::new("chat.message", &group.group_id);
        current.by = "user".into();
        current.data = json!({"to":["peer1"],"text":"current","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("current data");
        ledger::append(&ledger_path, &current).expect("current message");

        let pending = pending_sources(&home, &group, &actor, 10).expect("pending sources");

        assert_eq!(
            pending
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            [current_notice.id.as_str(), current.id.as_str()]
        );
    }

    #[test]
    fn batch_claim_rejects_a_conflict_before_claiming_other_recipients() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("runtime delivery batch", "").expect("group");
        let first = Actor::new("peer1");
        let second = Actor::new("peer2");
        group.actors.extend([first.clone(), second.clone()]);
        store.save(&group).expect("save actors");

        assert_eq!(
            claim(&home, &group, &second, "source-1", "pty", false).expect("reserve second"),
            ClaimResult::Claimed
        );
        let requests = [(&first, "pty"), (&second, "pty")];
        let (claimed, states) =
            claim_deliveries(&home, &group, &requests, "source-1", false).expect("batch claim");

        assert!(!claimed);
        assert_eq!(states.get("peer2").map(String::as_str), Some("claimed"));
        assert_eq!(
            latest_state(&home, &group.group_id, "peer1", "source-1").expect("first state"),
            None
        );
    }
}

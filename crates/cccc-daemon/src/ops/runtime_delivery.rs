use cccc_contracts::{Actor, Event};
use cccc_core::fs::with_exclusive_lock;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
#[cfg(test)]
use serde_json::Map;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dispatch::OpError;

#[cfg(test)]
mod legacy_read_watermark;

#[cfg(test)]
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
    /// This attempt stopped before any native transport submission.
    Unsubmitted(&'a str),
    Ambiguous(&'a str),
    /// Never sent and not to be retried: a newer event supersedes it.
    Withdrawn(&'a str),
}

impl<'a> DeliveryOutcome<'a> {
    fn parts(self) -> (&'static str, &'a str) {
        match self {
            Self::Claimed => ("claimed", ""),
            Self::Accepted => ("accepted", ""),
            Self::Failed(reason) | Self::Unsubmitted(reason) => ("failed", reason),
            Self::Ambiguous(reason) => ("ambiguous", reason),
            Self::Withdrawn(reason) => ("withdrawn", reason),
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
    if matches!(outcome, DeliveryOutcome::Unsubmitted(_)) {
        event.data.insert("retryable".into(), json!(true));
    }
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
    let group = store.load(group_id).map_err(OpError::io)?;
    Ok(
        inbox::delivery_states(home, &group, &[actor_id.to_owned()], source_event_id)
            .map_err(OpError::io)?
            .remove(actor_id),
    )
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
    if std::env::var_os("CCCC_DELIVERY_QUERY_PROBE").is_some_and(|value| value == "1") {
        cccc_runtime::diagnostic(format_args!(
            "CCCC_DELIVERY_STATE_QUERY {}",
            json!({"group_id":group.group_id,"actors":deliveries.len()})
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let lock_path = delivery_lock_path(&ledger_path);
    with_exclusive_lock(&lock_path, || {
        let actors = deliveries
            .iter()
            .map(|(actor, _)| actor.id.clone())
            .collect::<Vec<_>>();
        let latest = inbox::delivery_states(home, group, &actors, source_event_id)?;
        let (claimable, mut states) = {
            let mut states = HashMap::new();
            let mut claimable = true;
            for (actor, _transport) in deliveries {
                let state = latest
                    .get(&actor.id)
                    .map(|state| state.0.clone())
                    .unwrap_or_default();
                states.insert(actor.id.clone(), state.clone());
                if matches!(state.as_str(), "claimed" | "accepted" | "withdrawn")
                    || (state == "ambiguous" && !force_ambiguous)
                {
                    claimable = false;
                    break;
                }
            }
            (claimable, states)
        };
        if !claimable {
            return Ok((false, states));
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

fn read_stranded_claims(path: &Path) -> std::io::Result<HashMap<(String, String), String>> {
    // Startup recovery needs only unresolved claims. Building the generic
    // Event index here would retain history before IPC even becomes ready.
    let mut pending = HashMap::<(String, String), String>::new();
    ledger::visit_snapshot_oldest_first(path, |event| {
        if event.kind == "actor.add" {
            let actor = event
                .data
                .get("actor")
                .and_then(Value::as_object)
                .and_then(|actor| actor.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !actor.is_empty() {
                pending.retain(|(id, _), _| id != actor);
            }
        } else if event.kind == "runtime.delivery" {
            let actor = event
                .data
                .get("actor_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let source = event
                .data
                .get("source_event_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !actor.is_empty() && !source.is_empty() {
                let key = (actor.to_owned(), source.to_owned());
                if event.data.get("state").and_then(Value::as_str) == Some("claimed") {
                    pending.insert(
                        key,
                        event
                            .data
                            .get("transport")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    );
                } else {
                    pending.remove(&key);
                }
            }
        }
        std::ops::ControlFlow::Continue(())
    })?;
    Ok(pending)
}

pub fn settle_stranded_claims(home: &HomeLayout, group: &GroupDoc) -> Result<usize, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    with_exclusive_lock(&delivery_lock_path(&ledger_path), || {
        let pending = read_stranded_claims(&ledger_path)?;
        // No outcome is appended unless the complete snapshot was read.
        let mut settled = 0;
        for ((actor_id, source_event_id), transport) in pending {
            let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
                continue;
            };
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

/// The caller has retired the previous transport and verified no active turn.
/// A preclaimed handoff is uncertain, never proof that it is safe to resubmit.
pub(super) fn settle_replaced_transport_claims(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    transport: &str,
) -> Result<usize, OpError> {
    settle_claims(
        home,
        group,
        Some((actor_id, transport)),
        "delivery mode changed before the claimed handoff recorded an outcome",
    )
}

fn settle_claims(
    home: &HomeLayout,
    group: &GroupDoc,
    replacement: Option<(&str, &str)>,
    reason: &str,
) -> Result<usize, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let ledger_path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    with_exclusive_lock(&delivery_lock_path(&ledger_path), || {
        let pending = read_stranded_claims(&ledger_path)?;
        let mut settled = 0;
        for ((actor_id, source_event_id), transport) in pending {
            let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
                continue;
            };
            if replacement.is_some_and(|(id, current)| actor_id != id || transport == current) {
                continue;
            }
            append_state(
                home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                &source_event_id,
                &transport,
                DeliveryOutcome::Ambiguous(reason),
            )
            .map_err(|error| std::io::Error::other(error.message))?;
            settled += 1;
        }
        Ok(settled)
    })
    .map_err(OpError::io)
}

/// Why a stale Mail notice is withdrawn instead of delivered.
pub const STALE_MAIL_NOTICE: &str =
    "Mail notice withdrawn: its Mail was read, answered, or delivered";

/// A Mail notice none of whose Mail still awaits `actor_id`. Every transport
/// checks this right before handing a notice over, retries included. A failed
/// check keeps the notice: a redundant reminder is safer than a lost one.
pub fn is_stale_mail_notice(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    event: &Event,
) -> bool {
    if event.kind != "system.notify"
        || event.data.get("kind").and_then(Value::as_str) != Some("mail_notice")
    {
        return false;
    }
    let mail = event
        .data
        .get("context")
        .and_then(|context| context.get("source_event_ids"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    cccc_core::mail_pending::still_awaited(home, group, actor_id, &mail)
        .is_ok_and(|awaited| awaited.is_empty())
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
    inbox::pending_deliveries(home, group, &actor.id, limit).map_err(OpError::io)
}

#[cfg(test)]
fn legacy_pending_sources(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    limit: usize,
) -> Result<Vec<Event>, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    ledger::inspect(&path, |events, _| {
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
                latest.insert(
                    source.to_owned(),
                    json!({"state":state,"retryable":event.data.get("retryable")}),
                );
            }
        }
        let mut pending = Vec::new();
        for event in generation_events {
            if event.by == actor.id {
                continue;
            }
            let state = latest
                .get(&event.id)
                .and_then(|value| value["state"].as_str())
                .unwrap_or_default();
            let retryable = state == "failed"
                && latest
                    .get(&event.id)
                    .is_some_and(|value| value["retryable"] == true);
            let addressed = if event.kind == "chat.message" {
                let mode = event
                    .data
                    .get("message_mode")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                (matches!(mode, "send" | "request_reply")
                    || (mode == "mail" && (state == "claimed" || retryable)))
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
                    event.data.get("target_actor_id").and_then(Value::as_str)
                        == Some(actor.id.as_str())
                } else {
                    inbox::is_for_actor(group, event, &actor.id)
                }
            } else {
                false
            };
            if !addressed {
                continue;
            }
            // Native transport rejections are explicit failures, not an
            // automatic retry loop that can starve subsequent messages.
            // message_deliver still allows an operator to retry them.
            if !matches!(state, "accepted" | "ambiguous" | "withdrawn")
                && (state != "failed" || actor.external_session_id.is_empty() || retryable)
            {
                pending.push(event.clone());
                if pending.len() >= limit.max(1) {
                    break;
                }
            }
        }
        pending
    })
    .map_err(OpError::io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    #[ignore = "operator-supplied copied real groups only"]
    fn real_group_delivery_queries_match_history() {
        let root = std::env::var_os("CCCC_REAL_REPLAY_HOME").expect("real copied home required");
        let home = HomeLayout::from_path(root).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let groups = store.list().expect("registered groups");
        assert!(
            !groups.is_empty(),
            "empty replay cannot prove delivery parity"
        );
        for meta in groups {
            let group = store.load(&meta.group_id).expect("group");
            let check = |stage: &str| {
                let mut queried = 0;
                for actor in &group.actors {
                    for limit in [1, 20, 256] {
                        let expected = legacy_pending_sources(&home, &group, actor, limit)
                            .expect("legacy pending");
                        let actual = pending_sources(&home, &group, actor, limit)
                            .expect("projected pending");
                        assert_eq!(
                            serde_json::to_value(actual).expect("actual"),
                            serde_json::to_value(expected).expect("expected"),
                            "pending delivery differs from history"
                        );
                        queried += 1;
                    }
                }
                let latest = ledger::inspect(
                    &store.ledger_path(&group.group_id).expect("ledger"),
                    |events, _| {
                        let mut latest = std::collections::BTreeMap::new();
                        for event in events
                            .iter()
                            .filter(|event| event.kind == "runtime.delivery")
                        {
                            if let (Some(actor), Some(source)) = (
                                event.data.get("actor_id").and_then(Value::as_str),
                                event.data.get("source_event_id").and_then(Value::as_str),
                            ) {
                                latest.insert(
                                    (actor.to_owned(), source.to_owned()),
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
                                    ),
                                );
                            }
                        }
                        latest
                    },
                )
                .expect("legacy latest");
                let mut checked = 0;
                for ((actor, source), state) in latest.iter().take(256) {
                    assert_eq!(
                        latest_state(&home, &group.group_id, actor, source)
                            .expect("projected latest"),
                        Some(state.clone())
                    );
                    checked += 1;
                }
                println!(
                    "{}",
                    json!({"group_id":group.group_id,"pending_queries":queried,"latest_queries":checked,"stage":stage})
                );
            };
            check("cold");
            // Replay actual captured records, never padding or generated bodies.
            let path = store.ledger_path(&group.group_id).expect("ledger");
            let mut seeds = std::collections::BTreeMap::new();
            ledger::visit_snapshot_oldest_first(&path, |event| {
                let mode = event
                    .data
                    .get("message_mode")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(
                    event.kind.as_str(),
                    "actor.add" | "chat.read" | "runtime.delivery" | "system.notify"
                ) || (event.kind == "chat.message"
                    && matches!(mode, "mail" | "send" | "request_reply"))
                {
                    seeds.insert((event.kind.clone(), mode.to_owned()), event);
                }
                std::ops::ControlFlow::Continue(())
            })
            .expect("captured replay seeds");
            assert!(!seeds.is_empty(), "real delivery records required");
            for seed in seeds.values() {
                let mut replay = Event::new(&seed.kind, &group.group_id);
                replay.by = seed.by.clone();
                replay.scope_key = seed.scope_key.clone();
                replay.data = seed.data.clone();
                ledger::append(&path, &replay).expect("replay actual event");
            }
            check("append");
            if let Some(actor) = group.actors.iter().find(|actor| actor.enabled) {
                inbox::consume_unread(&home, &group, &actor.id, "user", 1)
                    .expect("durable real Mail cursor");
            }
            check("cursor");
            let seed = seeds.values().next().expect("seed");
            let mut external = Event::new(&seed.kind, &group.group_id);
            external.by = seed.by.clone();
            external.scope_key = seed.scope_key.clone();
            external.data = seed.data.clone();
            let mut bytes = serde_json::to_vec(&external).expect("external event");
            bytes.push(b'\n');
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("external writer")
                .write_all(&bytes)
                .expect("external growth without callback");
            check("external-growth");
            cccc_core::ledger_archive::compact(
                &home,
                &group.group_id,
                "real delivery parity replay",
            )
            .expect("real-history rotation");
            check("rotation");
        }
    }

    #[test]
    #[ignore = "operator-supplied copied real groups only"]
    fn real_group_startup_claims_match_history() {
        let root = std::env::var_os("CCCC_REAL_REPLAY_HOME").expect("real copied home required");
        let home = HomeLayout::from_path(root).expect("home");
        let store = GroupStore::new(home).expect("store");
        let groups = store.list().expect("registered real groups");
        assert!(
            !groups.is_empty(),
            "empty replay cannot prove claim recovery"
        );
        for group in groups {
            let path = store.ledger_path(&group.group_id).expect("ledger");
            // Preserve the old indexed reducer as a private-data oracle. It
            // deliberately retains terminal states before selecting claims.
            let expected = ledger::inspect(&path, |events, _| {
                let mut latest = HashMap::<(String, String), (String, String)>::new();
                for event in events {
                    if event.kind == "actor.add" {
                        let actor = event
                            .data
                            .get("actor")
                            .and_then(Value::as_object)
                            .and_then(|actor| actor.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !actor.is_empty() {
                            latest.retain(|(id, _), _| id != actor);
                        }
                    } else if event.kind == "runtime.delivery" {
                        let actor = event
                            .data
                            .get("actor_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let source = event
                            .data
                            .get("source_event_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !actor.is_empty() && !source.is_empty() {
                            latest.insert(
                                (actor.to_owned(), source.to_owned()),
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
                                ),
                            );
                        }
                    }
                }
                latest
                    .into_iter()
                    .filter_map(|(key, (state, transport))| {
                        (state == "claimed").then_some((key, transport))
                    })
                    .collect::<HashMap<_, _>>()
            })
            .expect("legacy claim reducer");
            let actual = read_stranded_claims(&path).expect("streamed claims");
            assert_eq!(
                actual, expected,
                "startup claim reducer differs from history"
            );
            println!(
                "{}",
                json!({"group_id":group.group_id,"stranded_claims":actual.len()})
            );
        }
    }

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

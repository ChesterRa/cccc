//! Bounded active work; one unavailable peer never blocks another peer's queue.
use super::{confirm_peer_scoped, exchange};
use crate::{dispatch_concurrency::DispatchLocks, ops::connect_outbound};
use base64::Engine;
use cccc_contracts::{Event, connect::ConnectPeerOperation, connect_message::*};
use cccc_core::{GroupStore, HomeLayout, connect_delivery, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use tokio::{task::JoinSet, time::Instant};

pub(super) async fn run(home: HomeLayout, client: super::PeerClient, locks: DispatchLocks) {
    let mut work = JoinSet::new();
    let mut active: HashMap<tokio::task::Id, (String, String)> = HashMap::new();
    let mut due: HashMap<(String, String), Instant> = HashMap::new();
    let mut peer_delays: HashMap<String, (u32, Instant)> = HashMap::new();
    let mut last_peer: Option<String> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(completed) = work.join_next_with_id(), if !work.is_empty() => {
                let task_id=completed.as_ref().map_or_else(|error|error.id(),|(id,_)|*id);
                if let Some(key) = active.remove(&task_id) {
                    match completed {
                        Ok((_,Ok(delay))) => {
                            peer_delays.remove(&key.0);
                            due.insert(key,Instant::now()+delay);
                        }
                        Ok((_,Err(DeliveryError::Local(error)))) => {
                            tracing::warn!(peer=%key.0, delivery=%key.1,%error,"Connect local delivery needs recovery");
                            due.insert(key,Instant::now()+Duration::from_secs(60));
                        }
                        result => {
                            let failure=peer_delays.entry(key.0.clone()).or_insert((0,Instant::now()));
                            failure.0=failure.0.saturating_add(1);
                            failure.1=Instant::now()+Duration::from_secs((5_u64<<failure.0.saturating_sub(1).min(4)).min(60));
                            due.insert(key.clone(),failure.1);
                            tracing::debug!(peer=%key.0, delivery=%key.1, error=?result,"Connect delivery deferred");
                        }
                    }
                }
            }
            _=tick.tick()=> {}
        }
        // The timer discovers new durable work. A completed worker also releases
        // capacity immediately; ready messages do not each pay the poll interval.
        let mut ids = match connect_delivery::pending_ids(&home) {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!(%error, "Connect outbox enumeration failed");
                continue;
            }
        };
        let present: HashSet<_> = ids.iter().cloned().collect();
        due.retain(|key, _| present.contains(key));
        peer_delays.retain(|peer, _| ids.iter().any(|(id, _)| id == peer));
        // pending_ids is sorted by peer. Resume after the last admitted peer so
        // a busy destination cannot continually reclaim a newly available slot.
        let start = last_peer
            .as_ref()
            .map_or(0, |peer| ids.partition_point(|(id, _)| id <= peer));
        ids.rotate_left(start);
        for key in ids {
            if active.len() >= 4 {
                break;
            }
            if active.values().any(|active_key| active_key.0 == key.0)
                || due.get(&key).is_some_and(|time| *time > Instant::now())
                || peer_delays
                    .get(&key.0)
                    .is_some_and(|(_, time)| *time > Instant::now())
            {
                continue;
            }
            let home = home.clone();
            let client = client.clone();
            let locks = locks.clone();
            let task_key = key.clone();
            let task = work.spawn(async move {
                process(&home, &client, &locks, &task_key.0, &task_key.1).await
            });
            last_peer = Some(key.0.clone());
            active.insert(task.id(), key);
        }
    }
}

pub(crate) async fn process(
    home: &HomeLayout,
    client: &super::PeerClient,
    locks: &DispatchLocks,
    peer: &str,
    id: &str,
) -> Result<Duration, DeliveryError> {
    let Some(mut entry) =
        connect_delivery::load(home, peer, id).map_err(|error| error.to_string())?
    else {
        return Ok(Duration::ZERO);
    };
    // Only local durable projections hold a dispatcher permit. HTTP never does.
    {
        let _permit = tokio::time::timeout(
            Duration::from_secs(5),
            locks.group_write(&entry.work.source().group_id),
        )
        .await
        .map_err(|_| "source Group is busy")?;
        if source_deleted(home, &entry)? {
            connect_delivery::remove_finalized(home, peer, id)
                .map_err(|error| error.to_string())?;
            return Ok(Duration::ZERO);
        }
        if let Some(final_event) = terminal_status(home, &entry)? {
            connect_outbound::notify_delivery_failure(home, &entry, &final_event)
                .map_err(|error| error.message)?;
            connect_delivery::remove_finalized(home, peer, id)
                .map_err(|error| error.to_string())?;
            return Ok(Duration::ZERO);
        }
        connect_outbound::project_source(home, &mut entry).map_err(|error| error.message)?;
    }
    let deadline = chrono::DateTime::parse_from_rfc3339(entry.work.deliver_before())
        .map_err(|error| error.to_string())?;
    let expired = deadline <= chrono::Utc::now();
    let retired = !expired
        && entry
            .work
            .connection_id()
            .map(|id| cccc_core::connect_groups::retired(home, id))
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
    if expired || retired {
        let status = if entry.progress.needs_receipt {
            "unconfirmed"
        } else {
            "failed"
        };
        finalize(
            home,
            locks,
            &entry,
            status,
            None,
            Some(if retired {
                "external Group connection was disconnected"
            } else {
                "delivery window ended"
            }),
        )
        .await?;
        return Ok(Duration::ZERO);
    }
    if let Some(next) = entry
        .progress
        .next_attempt_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    {
        let remaining = next.signed_duration_since(chrono::Utc::now());
        if remaining > chrono::Duration::zero() {
            return Ok(remaining.to_std().unwrap_or(Duration::from_secs(60)));
        }
    }
    let result = attempt(home, client, &mut entry).await;
    match result {
        Ok(Attempt::Delivered(receipt)) => {
            finalize(home, locks, &entry, "sent", Some(&receipt), None).await?
        }
        Ok(Attempt::Waiting) => return Ok(Duration::from_secs(5)),
        Ok(Attempt::Rejected(message)) => {
            finalize(home, locks, &entry, "failed", None, Some(&message)).await?
        }
        Err(error) => {
            entry.progress.attempts = entry.progress.attempts.saturating_add(1);
            let seconds = (5_i64 << entry.progress.attempts.saturating_sub(1).min(4)).min(60);
            entry.progress.next_attempt_at =
                Some((chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339());
            entry.progress.last_error = Some(error.to_string());
            connect_delivery::update_progress(home, peer, id, entry.progress)
                .map_err(|error| error.to_string())?;
            return Err(error);
        }
    }
    Ok(Duration::ZERO)
}

enum Attempt {
    Waiting,
    Delivered(ConnectDeliveryReceipt),
    Rejected(String),
}

async fn attempt(
    home: &HomeLayout,
    client: &super::PeerClient,
    entry: &mut ConnectOutboxEntry,
) -> Result<Attempt, DeliveryError> {
    connect_delivery::work_binding(home, &entry.work).map_err(DeliveryError::Peer)?;
    let ConnectWork::Message(message) = &entry.work else {
        return attempt_cancellation(home, client, entry).await;
    };
    let binding = confirm_peer_scoped(
        home,
        client,
        &message.target.instance_id,
        message.connection_id.as_deref(),
    )
    .await
    .map_err(DeliveryError::Peer)?;
    if entry.progress.needs_receipt {
        let result = exchange(
            home,
            client,
            &binding,
            ConnectPeerOperation::Receipt {
                connection_id: message.connection_id.clone(),
                source_group_id: message.source.group_id.clone(),
                target_group_id: message.target.group_id.clone(),
                delivery_id: message.delivery_id.clone(),
                message_sha256: connect_delivery::digest(message),
            },
            8192,
        )
        .await
        .map_err(DeliveryError::Peer)?;
        if result["ok"] != true {
            return Err(DeliveryError::Peer(
                "original delivery receipt is temporarily unavailable".into(),
            ));
        }
        if !result["result"]["receipt"].is_null() {
            return checked_receipt(message, &result["result"]["receipt"])
                .map_err(DeliveryError::Peer);
        }
        // An authenticated missing receipt establishes that a new attempt is safe.
        entry.progress.needs_receipt = false;
    }
    let mut blobs = Vec::new();
    let mut seen = HashSet::new();
    for file in &message.attachments {
        if !seen.insert(&file.sha256) {
            continue;
        }
        let path = cccc_core::blobs::resolve(home, &message.source.group_id, &file.sha256)
            .map_err(|error| error.to_string())?;
        if std::fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .len()
            != file.bytes
        {
            return Ok(Attempt::Rejected("source attachment changed".into()));
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        if format!("{:x}", Sha256::digest(&bytes)) != file.sha256 {
            return Ok(Attempt::Rejected("source attachment changed".into()));
        }
        blobs.push(ConnectBlobPayload {
            sha256: file.sha256.clone(),
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    // Commit the uncertainty checkpoint before a POST can escape this process.
    entry.progress.needs_receipt = true;
    connect_delivery::update_progress(
        home,
        &message.target.instance_id,
        &message.delivery_id,
        entry.progress.clone(),
    )
    .map_err(|error| error.to_string())?;
    let result = exchange(
        home,
        client,
        &binding,
        ConnectPeerOperation::Deliver {
            message: message.clone(),
            blobs,
        },
        8192,
    )
    .await
    .map_err(DeliveryError::Peer)?;
    if result["ok"] != true {
        let code = result["error"]["code"].as_str().unwrap_or("");
        if matches!(code, "io_error" | "connect_peer_denied") {
            return Err(DeliveryError::Peer(
                "peer could not confirm delivery".into(),
            ));
        }
        return Ok(Attempt::Rejected(
            result["error"]["message"]
                .as_str()
                .unwrap_or("peer rejected the message")
                .into(),
        ));
    }
    checked_receipt(message, &result["result"]["receipt"]).map_err(DeliveryError::Peer)
}

async fn attempt_cancellation(
    home: &HomeLayout,
    client: &super::PeerClient,
    entry: &mut ConnectOutboxEntry,
) -> Result<Attempt, DeliveryError> {
    let ConnectWork::Cancel(cancel) = &entry.work else {
        unreachable!("cancellation branch")
    };
    let binding = confirm_peer_scoped(
        home,
        client,
        &cancel.target.instance_id,
        cancel.connection_id.as_deref(),
    )
    .await
    .map_err(DeliveryError::Peer)?;
    entry.progress.needs_receipt = true;
    connect_delivery::update_progress(
        home,
        &cancel.target.instance_id,
        &cancel.delivery_id,
        entry.progress.clone(),
    )
    .map_err(|e| e.to_string())?;
    let response = exchange(
        home,
        client,
        &binding,
        ConnectPeerOperation::Cancel {
            cancellation: cancel.clone(),
        },
        8192,
    )
    .await
    .map_err(DeliveryError::Peer)?;
    if response["ok"] != true {
        let code = response["error"]["code"].as_str().unwrap_or("");
        if code == "connect_original_pending" {
            // A control preceding its original message must not back off the peer:
            // the original may be waiting in this very same peer's queue.
            entry.progress.needs_receipt = false;
            entry.progress.next_attempt_at =
                Some((chrono::Utc::now() + chrono::Duration::seconds(5)).to_rfc3339());
            connect_delivery::update_progress(
                home,
                &cancel.target.instance_id,
                &cancel.delivery_id,
                entry.progress.clone(),
            )
            .map_err(|e| e.to_string())?;
            return Ok(Attempt::Waiting);
        }
        if matches!(code, "io_error" | "connect_peer_denied") {
            return Err(DeliveryError::Peer(
                "peer could not confirm cancellation".into(),
            ));
        }
        return Ok(Attempt::Rejected(
            response["error"]["message"]
                .as_str()
                .unwrap_or("peer rejected cancellation")
                .into(),
        ));
    }
    checked_receipt_of(
        &cancel.delivery_id,
        &connect_delivery::cancellation_digest(cancel),
        &response["result"]["receipt"],
    )
    .map_err(DeliveryError::Peer)
}

fn checked_receipt(message: &ConnectMessage, value: &Value) -> Result<Attempt, String> {
    checked_receipt_of(
        &message.delivery_id,
        &connect_delivery::digest(message),
        value,
    )
}

fn checked_receipt_of(id: &str, digest: &str, value: &Value) -> Result<Attempt, String> {
    let receipt: ConnectDeliveryReceipt =
        serde_json::from_value(value.clone()).map_err(|_| "invalid delivery receipt")?;
    if receipt.delivery_id != id
        || receipt.message_sha256 != digest
        || uuid::Uuid::parse_str(&receipt.event_id).is_err()
        || chrono::DateTime::parse_from_rfc3339(&receipt.delivered_at).is_err()
    {
        return Err("delivery receipt does not match the accepted work".into());
    }
    Ok(Attempt::Delivered(receipt))
}

fn terminal_status(home: &HomeLayout, entry: &ConnectOutboxEntry) -> Result<Option<Event>, String> {
    let path = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(&entry.work.source().group_id))
        .map_err(|error| error.to_string())?;
    ledger::find_idempotent(
        &path,
        "chat.cross_group_receipt",
        "system",
        &format!("connect:final:{}", entry.work.delivery_id()),
    )
    .map_err(|error| error.to_string())
}

async fn finalize(
    home: &HomeLayout,
    locks: &DispatchLocks,
    entry: &ConnectOutboxEntry,
    status: &str,
    receipt: Option<&ConnectDeliveryReceipt>,
    error: Option<&str>,
) -> Result<(), String> {
    let _permit = tokio::time::timeout(
        Duration::from_secs(5),
        locks.group_write(&entry.work.source().group_id),
    )
    .await
    .map_err(|_| "source Group is busy")?;
    if !source_deleted(home, entry)? {
        let event = if let Some(existing) = terminal_status(home, entry)? {
            existing
        } else {
            let mut event = Event::new("chat.cross_group_receipt", &entry.work.source().group_id);
            event.by = "system".into();
            event.scope_key = entry.source_event.scope_key.clone();
            event.data=json!({"client_id":format!("connect:final:{}",entry.work.delivery_id()),"transport":"connect","action":if matches!(entry.work,ConnectWork::Cancel(_)) {"cancel"} else {"message"},"original_event_id":entry.source_event.data.get("source_event_id"),"delivery_id":entry.work.delivery_id(),"source_event_id":entry.work.source_event_id(),"dst_group_id":entry.work.target().group_id,"dst_instance_id":entry.work.target().instance_id,"status":status,"remote_event_id":receipt.map(|value|&value.event_id),"error":error,"receipt":receipt}).as_object().expect("object").clone();
            let path = GroupStore::new(home.clone())
                .and_then(|store| store.ledger_path(&event.group_id))
                .map_err(|error| error.to_string())?;
            ledger::append(&path, &event).map_err(|error| error.to_string())?;
            event
        };
        connect_outbound::notify_delivery_failure(home, entry, &event)
            .map_err(|error| error.message)?;
    }
    connect_delivery::remove_finalized(
        home,
        &entry.work.target().instance_id,
        entry.work.delivery_id(),
    )
    .map_err(|error| error.to_string())
}

#[derive(Debug)]
pub(crate) enum DeliveryError {
    Local(String),
    Peer(String),
}
impl From<String> for DeliveryError {
    fn from(value: String) -> Self {
        Self::Local(value)
    }
}
impl From<&str> for DeliveryError {
    fn from(value: &str) -> Self {
        Self::Local(value.into())
    }
}
impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(value) | Self::Peer(value) => f.write_str(value),
        }
    }
}

fn source_deleted(home: &HomeLayout, entry: &ConnectOutboxEntry) -> Result<bool, String> {
    let group_id = &entry.work.source().group_id;
    if cccc_core::Registry::load(home)
        .map_err(|error| error.to_string())?
        .groups
        .contains_key(group_id)
    {
        return Ok(false);
    }
    let directory = GroupStore::new(home.clone())
        .and_then(|store| store.group_dir(group_id))
        .map_err(|error| error.to_string())?;
    if directory.try_exists().map_err(|error| error.to_string())? {
        return Err("source Group registration is missing; retaining accepted work".into());
    }
    Ok(true)
}

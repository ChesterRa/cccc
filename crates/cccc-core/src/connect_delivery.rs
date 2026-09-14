//! Active delivery persistence. Final receipts remain in each Group's ledger.
use crate::{HomeLayout, connect_peer, fs};
use cccc_contracts::connect_message::{
    ConnectCancellation, ConnectDeliveryProgress, ConnectMessage, ConnectOutboxEntry, ConnectWork,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io, path::PathBuf};

pub const DELIVERY_SECONDS: i64 = 15 * 60;
pub const REPLY_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_PENDING: usize = 1024;
pub const MAX_PENDING_PER_PEER: usize = 128;
const MAX_PENDING_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PENDING_PEER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 512 * 1024;

#[cfg(test)]
#[path = "connect_delivery_tests.rs"]
mod tests;

pub fn digest(message: &ConnectMessage) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(message).expect("serializable Connect message"))
    )
}

pub fn validate(message: &ConnectMessage) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid Connect message");
    let created = DateTime::parse_from_rfc3339(&message.created_at).map_err(|_| invalid())?;
    let deadline = DateTime::parse_from_rfc3339(&message.deliver_before).map_err(|_| invalid())?;
    let reply_deadline =
        DateTime::parse_from_rfc3339(&message.reply_before).map_err(|_| invalid())?;
    if uuid::Uuid::parse_str(&message.delivery_id).is_err()
        || uuid::Uuid::parse_str(&message.source_event_id).is_err()
        || message.source.instance_id == message.target.instance_id
        || created > Utc::now() + chrono::Duration::seconds(30)
        || deadline <= created
        || deadline - created > chrono::Duration::seconds(DELIVERY_SECONDS)
        || reply_deadline < deadline
        || reply_deadline - created > chrono::Duration::seconds(REPLY_SECONDS)
        || message.text.len() > 64 * 1024
        || (message.text.trim().is_empty() && message.attachments.is_empty())
        || !matches!(message.format.as_str(), "plain" | "markdown")
        || !matches!(
            message.message_mode.as_str(),
            "send" | "request_reply" | "mail"
        )
        || message.recipients.is_empty()
        || message.recipients.len() > 128
        || message.attachments.len() > 16
    {
        return Err(invalid());
    }
    for address in [&message.source, &message.target] {
        if address.group_id.is_empty()
            || address.group_id.len() > 128
            || address.group_id.contains(['/', '\\'])
            || address.title.len() > 4096
        {
            return Err(invalid());
        }
    }
    let mut recipients = HashSet::new();
    for actor in std::iter::once(&message.sender).chain(&message.recipients) {
        if actor.title.len() > 4096
            || actor.generation.len() > 128
            || (actor.id == "user" && !actor.generation.is_empty())
            || (actor.id != "user"
                && (crate::actors::validate_actor_id(&actor.id).is_err()
                    || actor.generation.is_empty()))
        {
            return Err(invalid());
        }
    }
    if message
        .recipients
        .iter()
        .any(|actor| !recipients.insert(&actor.id))
    {
        return Err(invalid());
    }
    let mut bytes = 0_u64;
    for attachment in &message.attachments {
        if attachment.sha256.len() != 64
            || !attachment
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || attachment.bytes > MAX_ATTACHMENT_BYTES
            || attachment.name.len() > 4096
            || attachment.mime_type.len() > 256
        {
            return Err(invalid());
        }
        bytes = bytes.saturating_add(attachment.bytes);
    }
    if bytes > MAX_ATTACHMENT_BYTES {
        return Err(invalid());
    }
    if let Some(reply) = &message.reply_to {
        if message.message_mode == "request_reply" {
            return Err(invalid());
        }
        if uuid::Uuid::parse_str(&reply.delivery_id).is_err()
            || uuid::Uuid::parse_str(&reply.event_id).is_err()
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub fn work_digest(work: &ConnectWork) -> String {
    match work {
        ConnectWork::Message(message) => digest(message),
        ConnectWork::Cancel(cancel) => cancellation_digest(cancel),
    }
}

pub fn cancellation_digest(cancel: &ConnectCancellation) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(cancel).expect("serializable cancellation"))
    )
}

pub fn validate_work(work: &ConnectWork) -> io::Result<()> {
    match work {
        ConnectWork::Message(message) => validate(message),
        ConnectWork::Cancel(cancel) => validate_cancellation(cancel),
    }
}

pub fn validate_cancellation(cancel: &ConnectCancellation) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid Connect cancellation");
    let created = DateTime::parse_from_rfc3339(&cancel.created_at).map_err(|_| invalid())?;
    let deadline = DateTime::parse_from_rfc3339(&cancel.deliver_before).map_err(|_| invalid())?;
    DateTime::parse_from_rfc3339(&cancel.original_deliver_before).map_err(|_| invalid())?;
    if [
        &cancel.delivery_id,
        &cancel.source_event_id,
        &cancel.original_delivery_id,
        &cancel.original_source_event_id,
    ]
    .iter()
    .any(|id| uuid::Uuid::parse_str(id).is_err())
        || cancel.source.instance_id == cancel.target.instance_id
        || cancel.original_message_sha256.len() != 64
        || !cancel
            .original_message_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || created > Utc::now() + chrono::Duration::seconds(30)
        || deadline <= created
        || deadline - created > chrono::Duration::seconds(DELIVERY_SECONDS)
        || [&cancel.source, &cancel.target].iter().any(|address| {
            address.group_id.is_empty()
                || address.group_id.len() > 128
                || address.group_id.contains(['/', '\\'])
                || address.title.len() > 4096
        })
        || cancel.sender.title.len() > 4096
        || cancel.sender.generation.len() > 128
        || (cancel.sender.id == "user" && !cancel.sender.generation.is_empty())
        || (cancel.sender.id != "user"
            && (crate::actors::validate_actor_id(&cancel.sender.id).is_err()
                || cancel.sender.generation.is_empty()))
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn work_binding(home: &HomeLayout, work: &ConnectWork) -> Result<(), String> {
    let binding =
        connect_peer::scoped_binding(home, &work.target().instance_id, work.connection_id())?;
    if binding.group.as_ref().is_some_and(|g| {
        g.local_group_id != work.source().group_id || g.remote_group_id != work.target().group_id
    }) || binding.account_origin != work.account_origin()
        || binding.account_id != work.account_id()
        || binding.local.instance_id != work.source().instance_id
        || binding.local.device_id != work.source().device_id
        || binding.remote.device_id != work.target().device_id
    {
        return Err("the delivery belongs to a previous membership binding".into());
    }
    Ok(())
}

/// A reply stays within the original qualified Group pair and participant set.
/// The participating Group's human user may answer as user, never as an Actor.
pub fn validate_reply(original: &ConnectMessage, reply: &ConnectMessage) -> Result<(), String> {
    let same_address =
        |a: &cccc_contracts::connect_message::ConnectGroupAddress,
         b: &cccc_contracts::connect_message::ConnectGroupAddress| {
            a.instance_id == b.instance_id && a.device_id == b.device_id && a.group_id == b.group_id
        };
    let same_actor = |a: &cccc_contracts::connect::ConnectActor,
                      b: &cccc_contracts::connect::ConnectActor| {
        a.id == b.id && a.generation == b.generation
    };
    let reverse = same_address(&original.source, &reply.target)
        && same_address(&original.target, &reply.source);
    let forward = same_address(&original.source, &reply.source)
        && same_address(&original.target, &reply.target);
    let sender_allowed = reply.sender.id == "user"
        || if reverse {
            original
                .recipients
                .iter()
                .any(|actor| same_actor(actor, &reply.sender))
        } else {
            same_actor(&original.sender, &reply.sender)
        };
    let recipients_allowed = if reverse {
        reply.recipients.len() == 1 && same_actor(&original.sender, &reply.recipients[0])
    } else {
        !reply.recipients.is_empty()
            && reply.recipients.iter().all(|recipient| {
                original
                    .recipients
                    .iter()
                    .any(|actor| same_actor(actor, recipient))
            })
    };
    if (!reverse && !forward)
        || !sender_allowed
        || !recipients_allowed
        || original.account_origin != reply.account_origin
        || original.account_id != reply.account_id
        || original.connection_id != reply.connection_id
        || reply
            .reply_to
            .as_ref()
            .is_none_or(|reference| reference.delivery_id != original.delivery_id)
        || DateTime::parse_from_rfc3339(&original.reply_before)
            .map_err(|_| "invalid original reply deadline")?
            <= Utc::now()
    {
        return Err(
            "reply must stay within the original Group pair, participants and reply window".into(),
        );
    }
    Ok(())
}

fn root(home: &HomeLayout) -> PathBuf {
    home.root().join("state/connect/outbox")
}
fn path(home: &HomeLayout, peer: &str, id: &str) -> io::Result<PathBuf> {
    if !(16..=128).contains(&peer.len())
        || !peer.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || uuid::Uuid::parse_str(id).is_err()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid delivery identity",
        ));
    }
    Ok(root(home).join(peer).join(format!("{id}.json")))
}

pub fn load(home: &HomeLayout, peer: &str, id: &str) -> io::Result<Option<ConnectOutboxEntry>> {
    let path = path(home, peer, id)?;
    let metadata = match std::fs::metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.len() > MAX_RECORD_BYTES {
        return Err(io::Error::other("Connect outbox record exceeds its limit"));
    }
    let entry: ConnectOutboxEntry = fs::read_json(&path)?;
    if entry.work.delivery_id() != id || entry.work.target().instance_id != peer {
        return Err(io::Error::other("Connect outbox identity mismatch"));
    }
    Ok(Some(entry))
}

/// Directory enumeration is cheap; workers only load records that are due.
pub fn pending_ids(home: &HomeLayout) -> io::Result<Vec<(String, String)>> {
    let entries = match std::fs::read_dir(root(home)) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let peer = entry.file_name().to_string_lossy().into_owned();
        for file in std::fs::read_dir(entry.path())? {
            let file = file?;
            if !file.file_type()?.is_file() {
                continue;
            }
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            path(home, &peer, id)?;
            ids.push((peer.clone(), id.into()));
        }
    }
    ids.sort();
    Ok(ids)
}

/// Persist before acknowledging acceptance. A retry cannot change an accepted body.
pub fn reserve(home: &HomeLayout, entry: &ConnectOutboxEntry) -> io::Result<ConnectOutboxEntry> {
    validate_work(&entry.work)?;
    work_binding(home, &entry.work)
        .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
    let source_kind = match &entry.work {
        ConnectWork::Message(_) => "chat.message",
        ConnectWork::Cancel(_) => "chat.reply_request.cancelled",
    };
    if entry.source_event.kind != source_kind
        || entry.source_event.id != entry.work.source_event_id()
        || entry.source_event.group_id != entry.work.source().group_id
        || entry.source_event.by != entry.work.sender().id
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source projection does not match the delivery",
        ));
    }
    if serde_json::to_vec(entry).map_err(io::Error::other)?.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Connect outbox record exceeds its limit",
        ));
    }
    fs::with_exclusive_lock(&root(home).join("outbox.lock"), || {
        let peer = &entry.work.target().instance_id;
        let id = entry.work.delivery_id();
        if let Some(existing) = load(home, peer, id)? {
            if work_digest(&existing.work) != work_digest(&entry.work)
                || existing.source_event != entry.source_event
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "delivery ID already accepted with different content",
                ));
            }
            return Ok(existing);
        }
        let ids = pending_ids(home)?;
        if ids.iter().any(|(_, queued_id)| queued_id == id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "delivery ID already accepted for another peer",
            ));
        }
        if ids.len() >= MAX_PENDING
            || ids.iter().filter(|(target, _)| target == peer).count() >= MAX_PENDING_PER_PEER
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Connect pending delivery limit reached",
            ));
        }
        let new_bytes = entry
            .work
            .attachments()
            .iter()
            .map(|file| file.bytes)
            .sum::<u64>();
        let mut total = new_bytes;
        let mut peer_total = new_bytes;
        for (target, id) in ids {
            // An unreadable record still consumes its full possible file budget.
            // Preserve it for recovery without blocking unrelated new messages.
            let bytes = match load(home, &target, &id) {
                Ok(Some(queued)) => queued
                    .work
                    .attachments()
                    .iter()
                    .map(|file| file.bytes)
                    .sum::<u64>(),
                Ok(None) => 0,
                Err(_) => MAX_ATTACHMENT_BYTES,
            };
            total = total.saturating_add(bytes);
            if target == *peer {
                peer_total = peer_total.saturating_add(bytes);
            }
        }
        if total > MAX_PENDING_BYTES || peer_total > MAX_PENDING_PEER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Connect pending attachment limit reached",
            ));
        }
        fs::write_json_committed_with(&path(home, peer, id)?, entry, fs::write_secret_json)?;
        Ok(entry.clone())
    })
}

pub fn update_progress(
    home: &HomeLayout,
    peer: &str,
    id: &str,
    progress: ConnectDeliveryProgress,
) -> io::Result<()> {
    fs::with_exclusive_lock(&root(home).join("outbox.lock"), || {
        let mut entry = load(home, peer, id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pending delivery not found"))?;
        entry.progress = progress;
        fs::write_json_committed_with(&path(home, peer, id)?, &entry, fs::write_secret_json)
    })
}

/// The caller must first durably append the final status to the source ledger.
pub fn remove_finalized(home: &HomeLayout, peer: &str, id: &str) -> io::Result<()> {
    fs::with_exclusive_lock(
        &root(home).join("outbox.lock"),
        || match std::fs::remove_file(path(home, peer, id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        },
    )
}

/// Only the original sender (same generation), or a participating Group's user,
/// can withdraw the original request. The receiver verifies the same relationship.
pub fn validate_cancellation_for(
    original: &ConnectMessage,
    cancel: &ConnectCancellation,
) -> Result<(), String> {
    let same = |a: &cccc_contracts::connect_message::ConnectGroupAddress,
                b: &cccc_contracts::connect_message::ConnectGroupAddress| {
        a.instance_id == b.instance_id && a.device_id == b.device_id && a.group_id == b.group_id
    };
    let forward = same(&original.source, &cancel.source) && same(&original.target, &cancel.target);
    let reverse = same(&original.target, &cancel.source) && same(&original.source, &cancel.target);
    let actor_matches = original.sender.id == cancel.sender.id
        && original.sender.generation == cancel.sender.generation;
    if original.message_mode != "request_reply"
        || original.delivery_id != cancel.original_delivery_id
        || digest(original) != cancel.original_message_sha256
        || original.source_event_id != cancel.original_source_event_id
        || original.deliver_before != cancel.original_deliver_before
        || original.account_origin != cancel.account_origin
        || original.account_id != cancel.account_id
        || original.connection_id != cancel.connection_id
        || (!forward && !reverse)
        || (cancel.sender.id != "user" && (!forward || !actor_matches))
    {
        return Err("cancellation does not belong to the original request and participant".into());
    }
    Ok(())
}

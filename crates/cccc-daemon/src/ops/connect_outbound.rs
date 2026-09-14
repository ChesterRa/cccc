//! Durable local acceptance. No network operation runs under the source Group lock.
use super::{
    connect_messages, connect_peer,
    operation::{Operation, Policy::Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{
    Actor, DaemonRequest, Event,
    connect::{ConnectActor, ConnectGroup},
    connect_message::*,
};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, connect_catalog, connect_delivery, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    matches!(request.op.as_str(), "connect_send" | "connect_send_files")
        .then(|| Operation::new(Write, send))
}

fn send(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    send_with_reply(home, request, None, false)
}

struct ReplyContext {
    original: ConnectMessage,
    local_event: Event,
    target: ConnectGroupAddress,
}

fn send_with_reply(
    home: &HomeLayout,
    request: &DaemonRequest,
    reply: Option<ReplyContext>,
    preflight: bool,
) -> OpResult {
    let group_id = connect_peer::authorize_local_source(home, request)?;
    if request
        .args
        .get("refs")
        .is_some_and(|value| value.as_array().is_none_or(|refs| !refs.is_empty()))
        || request
            .args
            .get("suggested_user_message")
            .is_some_and(|value| !value.is_null())
    {
        return Err(OpError::new(
            "unsupported_connect_content",
            "remote messages accept text and attachments; local resource refs and composer suggestions are not shared",
        ));
    }
    if request.op == "connect_send" && request.args.contains_key("paths") {
        return Err(OpError::new(
            "invalid_paths",
            "use connect_send_files for source files",
        ));
    }

    let by = request
        .args
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let peer = required_arg(request, "instance_id")?;
    let target_group = required_arg(request, "target_group_id")?;
    let client_id = required_arg(request, "client_id")?;
    if client_id.len() > 128 {
        return Err(OpError::new(
            "invalid_client_id",
            "client_id must not exceed 128 bytes",
        ));
    }
    let binding = if let Some(reply) = &reply {
        cccc_core::connect_peer::scoped_binding(
            home,
            &peer,
            reply.original.connection_id.as_deref(),
        )
    } else {
        cccc_core::connect_peer::group_binding(home, &peer, &group_id, &target_group)
    }
    .map_err(|message| OpError::new("connect_peer_unavailable", message))?;
    let key = Sha256::digest(
        serde_json::to_vec(&json!([
            binding.local.instance_id,
            binding.local.device_id,
            group_id,
            by,
            client_id
        ]))
        .map_err(OpError::invalid)?,
    );
    let id = uuid::Uuid::from_slice(&key[..16])
        .map_err(OpError::invalid)?
        .to_string();
    let request_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([request.op, request.args])).map_err(OpError::invalid)?
        )
    );
    if !preflight
        && let Some(entry) = connect_delivery::load(home, &peer, &id).map_err(OpError::io)?
    {
        check_retry(&entry.source_event, &request_digest)?;
        return object(
            json!({"accepted":true,"queued":true,"delivery_state":"queued","delivery_id":id,"source_event":entry.source_event,"duplicate":true}),
        );
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let source_path = store.ledger_path(&group_id).map_err(OpError::io)?;
    if !preflight
        && let Some(event) = ledger::find_idempotent(&source_path, "chat.message", by, &client_id)
            .map_err(OpError::io)?
    {
        check_retry(&event, &request_digest)?;
        let receipt = ledger::find_idempotent(
            &source_path,
            "chat.cross_group_receipt",
            "system",
            &format!("connect:final:{id}"),
        )
        .map_err(OpError::io)?;
        let state = receipt
            .as_ref()
            .and_then(|event| event.data.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("unconfirmed");
        return object(
            json!({"accepted":true,"queued":false,"delivery_state":state,"receipt":receipt,"delivery_id":id,"source_event":event,"duplicate":true}),
        );
    }
    let source = store.load(&group_id).map_err(OpError::io)?;
    let target = if let Some(reply) = &reply {
        if binding.remote.device_id != reply.target.device_id {
            return Err(OpError::new(
                "connect_reply_denied",
                "original peer binding changed",
            ));
        }
        let actors = if reply.original.source.instance_id == peer {
            vec![reply.original.sender.clone()]
        } else {
            reply.original.recipients.clone()
        };
        ConnectGroup {
            group_id: reply.target.group_id.clone(),
            title: reply.target.title.clone(),
            actors: actors
                .into_iter()
                .filter(|actor| actor.id != "user")
                .collect(),
        }
    } else {
        connect_catalog::load_scoped(home, &peer, binding.group.as_ref().map(|g| g.id.as_str()))
            .map_err(OpError::io)?
            .and_then(|catalog| {
                catalog
                    .groups
                    .into_iter()
                    .find(|group| group.group_id == target_group)
            })
            .ok_or_else(|| {
                OpError::new(
                    "connect_catalog_unavailable",
                    "target Group is not in the known directory; wait for discovery",
                )
            })?
    };
    let mut data = request.args.clone();
    data.entry("to").or_insert_with(|| json!(["@foreman"]));
    if reply.is_some()
        && data
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|item| matches!(item.trim(), "@all" | "@foreman" | "@peers"))
            })
    {
        return Err(OpError::new(
            "connect_reply_denied",
            "omit to or use concrete original participants when replying",
        ));
    }
    super::messaging_recipients::normalize_remote_chat_data(&mut data)?;
    let recipients = recipient_snapshot(&target, &mut data)?;
    if request.op == "connect_send_files" {
        if request.args.contains_key("attachments") {
            return Err(OpError::new(
                "invalid_attachments",
                "connect_send_files owns attachment records",
            ));
        }
        let paths = request
            .args
            .get("paths")
            .and_then(Value::as_array)
            .filter(|paths| !paths.is_empty() && paths.len() <= 16)
            .ok_or_else(|| {
                OpError::new(
                    "invalid_paths",
                    "paths must contain 1–16 source-scope files",
                )
            })?;
        super::messaging::files::read(&source, paths, connect_delivery::MAX_ATTACHMENT_BYTES)?
            .apply(home, &source, &mut data)?;
    }
    let attachments = if preflight {
        Vec::new()
    } else {
        source_attachments(home, &group_id, data.get("attachments"))?
    };
    let now = chrono::Utc::now();
    let mut event = Event::new("chat.message", &group_id);
    event.by = by.into();
    event.scope_key = source.active_scope_key.clone();
    let message = ConnectMessage {
        connection_id: binding.group.as_ref().map(|g| g.id.clone()),
        delivery_id: id.clone(),
        account_origin: binding.account_origin,
        account_id: binding.account_id,
        source: ConnectGroupAddress {
            instance_id: binding.local.instance_id,
            device_id: binding.local.device_id,
            group_id: group_id.clone(),
            title: source.title.clone(),
        },
        target: ConnectGroupAddress {
            instance_id: peer.clone(),
            device_id: binding.remote.device_id,
            group_id: target.group_id,
            title: target.title,
        },
        sender: if by == "user" {
            user()
        } else {
            let actor = cccc_core::actors::find(&source, by)
                .ok_or_else(|| OpError::new("actor_not_found", "sender no longer exists"))?;
            ConnectActor {
                id: actor.id.clone(),
                title: actor.title.clone(),
                generation: connect_messages::actor_generation(actor),
                enabled: actor.enabled,
                role: cccc_core::actors::effective_role(&source, by),
            }
        },
        source_event_id: event.id.clone(),
        recipients,
        text: data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        format: data
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("plain")
            .into(),
        insight: data
            .get("insight")
            .and_then(Value::as_str)
            .map(str::to_owned),
        message_mode: data["message_mode"]
            .as_str()
            .expect("normalized mode")
            .into(),
        attachments,
        reply_to: data
            .get("connect_reply_to")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(OpError::invalid)?,
        created_at: now.to_rfc3339(),
        deliver_before: (now + chrono::Duration::seconds(connect_delivery::DELIVERY_SECONDS))
            .to_rfc3339(),
        reply_before: (now + chrono::Duration::seconds(connect_delivery::REPLY_SECONDS))
            .to_rfc3339(),
    };
    if !preflight {
        connect_delivery::validate(&message).map_err(OpError::invalid)?;
    }
    if let Some(reply) = &reply {
        connect_delivery::validate_reply(&reply.original, &message)
            .map_err(|message| OpError::new("connect_reply_denied", message))?;
    }

    if preflight {
        super::messaging::validate_upload_content(request, &data)?;
        return object(json!({"ready":true}));
    }

    event.data = json!({
        "text":message.text,"format":message.format,"insight":message.insight,"to":["user"],"message_mode":"send",
        "dst_group_id":message.target.group_id,"dst_group_title":message.target.title,"dst_instance_id":peer,"dst_to":message.recipients.iter().map(|actor|&actor.id).collect::<Vec<_>>(),"dst_message_mode":message.message_mode,
        "dst_instance_name":binding.remote.display_name,
        "dst_actor_titles":message.recipients.iter().map(|actor|(actor.id.clone(),json!(actor.title))).collect::<serde_json::Map<_,_>>(),
        "attachments":data.get("attachments").cloned().unwrap_or_else(||json!([])),
        "client_id":client_id,"connect_request_sha256":request_digest,"connect_message":message,
    }).as_object().expect("object").clone();
    if let Some(reply) = &reply {
        event
            .data
            .insert("reply_to".into(), json!(reply.local_event.id));
        super::message_metadata::add_reply_snapshot(&reply.local_event, &mut event.data);
    }
    super::message_metadata::add_sender_snapshot(&source, by, &mut event.data);
    let mut entry = connect_delivery::reserve(
        home,
        &ConnectOutboxEntry {
            work: ConnectWork::Message(Box::new(message)),
            source_event: event,
            progress: Default::default(),
        },
    )
    .map_err(OpError::io)?;
    // A source projection failure is recoverable from the already-accepted record.
    // Never report it as a failed send and invite a second delivery ID.
    let projected = project_source(home, &mut entry).is_ok();
    object(
        json!({"accepted":true,"queued":true,"delivery_state":"queued","delivery_id":id,"source_event":entry.source_event,"source_projected":projected}),
    )
}

fn check_retry(event: &Event, digest: &str) -> Result<(), OpError> {
    if event
        .data
        .get("connect_request_sha256")
        .and_then(Value::as_str)
        != Some(digest)
    {
        return Err(OpError::new(
            "connect_delivery_conflict",
            "client_id was already accepted with different parameters",
        ));
    }
    Ok(())
}

pub(crate) fn project_source(
    home: &HomeLayout,
    entry: &mut ConnectOutboxEntry,
) -> Result<(), OpError> {
    let path = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(&entry.work.source().group_id))
        .map_err(OpError::io)?;
    let existing = ledger::find_event(&path, &entry.source_event.id).map_err(OpError::io)?;
    if let Some(existing) = existing {
        if existing != entry.source_event {
            return Err(OpError::new(
                "connect_delivery_conflict",
                "source event changed",
            ));
        }
    } else {
        ledger::append(&path, &entry.source_event).map_err(OpError::io)?;
    }
    if !entry.progress.source_projected {
        entry.progress.source_projected = true;
        connect_delivery::update_progress(
            home,
            &entry.work.target().instance_id,
            entry.work.delivery_id(),
            entry.progress.clone(),
        )
        .map_err(OpError::io)?;
    }
    Ok(())
}

fn user() -> ConnectActor {
    ConnectActor {
        id: "user".into(),
        title: "User".into(),
        generation: String::new(),
        enabled: true,
        role: None,
    }
}

/// Adapt only the public recipient snapshot to the existing audience semantics.
/// This document is never persisted or used for runtime configuration.
fn recipient_snapshot(
    target: &ConnectGroup,
    data: &mut serde_json::Map<String, Value>,
) -> Result<Vec<ConnectActor>, OpError> {
    let group = GroupDoc {
        generation: String::new(),
        v: 1,
        group_id: target.group_id.clone(),
        title: target.title.clone(),
        topic: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
        running: false,
        state: cccc_contracts::GroupState::Active,
        active_scope_key: String::new(),
        scopes: vec![],
        automation: Default::default(),
        extra: Default::default(),
        actors: target
            .actors
            .iter()
            .map(|item| {
                let mut actor = Actor::new(&item.id);
                actor.title = item.title.clone();
                actor.enabled = item.enabled;
                actor.generation = item.generation.clone();
                actor
            })
            .collect(),
    };
    super::messaging_recipients::normalize_chat_preflight(&group, "connect:source", data, false)?;
    if data["to"]
        .as_array()
        .is_some_and(|to| to.iter().any(|id| id == "user"))
    {
        return Ok(vec![user()]);
    }
    let mut event = Event::new("chat.message", &target.group_id);
    event.by = "connect:source".into();
    event.data = data.clone();
    let recipients = target
        .actors
        .iter()
        .filter(|actor| cccc_core::inbox::is_for_actor(&group, &event, &actor.id))
        .cloned()
        .collect::<Vec<_>>();
    if recipients.is_empty() {
        return Err(OpError::new(
            "no_recipients",
            "target Group has no matching recipients",
        ));
    }
    Ok(recipients)
}

fn source_attachments(
    home: &HomeLayout,
    group: &str,
    raw: Option<&Value>,
) -> Result<Vec<ConnectAttachment>, OpError> {
    let Some(raw) = raw else {
        return Ok(vec![]);
    };
    let files = raw
        .as_array()
        .filter(|files| files.len() <= 16)
        .ok_or_else(|| {
            OpError::new(
                "invalid_attachments",
                "attachments must contain at most 16 blobs",
            )
        })?;
    let mut result = Vec::new();
    let mut total = 0_u64;
    for file in files {
        let path = file["path"]
            .as_str()
            .ok_or_else(|| OpError::new("invalid_attachments", "attachment path required"))?;
        let path = cccc_core::blobs::resolve(home, group, path).map_err(OpError::invalid)?;
        let bytes = std::fs::metadata(&path).map_err(OpError::io)?.len();
        total = total.saturating_add(bytes);
        if total > connect_delivery::MAX_ATTACHMENT_BYTES {
            return Err(OpError::new(
                "attachment_too_large",
                "Connect attachments exceed 10 MiB",
            ));
        }
        let content = std::fs::read(path).map_err(OpError::io)?;
        // Normalize legacy blob filenames to the content-addressed source used by retries.
        let blob = cccc_core::blobs::store(home, group, &content).map_err(OpError::io)?;
        result.push(ConnectAttachment {
            sha256: blob.sha256,
            bytes: blob.bytes as u64,
            name: file["title"].as_str().unwrap_or("file").into(),
            mime_type: file["mime_type"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .into(),
        });
    }
    Ok(result)
}

pub(super) fn reply(
    home: &HomeLayout,
    request: &DaemonRequest,
    group: &GroupDoc,
    target: &Event,
    mode: &str,
    preflight: bool,
) -> OpResult {
    let original = connect_messages::stored_message(target)?;
    let incoming = target
        .data
        .get("src_instance_id")
        .and_then(Value::as_str)
        .is_some();
    let destination = if incoming {
        original.source.clone()
    } else {
        original.target.clone()
    };
    let (recipients, remote_event) = if incoming {
        (
            vec![original.sender.id.clone()],
            original.source_event_id.clone(),
        )
    } else {
        let path = GroupStore::new(home.clone())
            .and_then(|store| store.ledger_path(&group.group_id))
            .map_err(OpError::io)?;
        let receipt = ledger::find_idempotent(
            &path,
            "chat.cross_group_receipt",
            "system",
            &format!("connect:final:{}", original.delivery_id),
        )
        .map_err(OpError::io)?;
        let remote_event=receipt.as_ref().filter(|event|event.data.get("status").and_then(Value::as_str)==Some("sent")).and_then(|event|event.data.get("remote_event_id")).and_then(Value::as_str).ok_or_else(||OpError::new("connect_delivery_pending","the original target event is not yet confirmed; wait for its receipt or send a new message"))?.to_owned();
        (
            original
                .recipients
                .iter()
                .map(|actor| actor.id.clone())
                .collect(),
            remote_event,
        )
    };
    let mut forwarded = request.clone();
    forwarded.args.remove("dst_group_id");
    forwarded.args.remove("dst_instance_id");
    forwarded
        .args
        .insert("instance_id".into(), json!(destination.instance_id));
    forwarded
        .args
        .insert("target_group_id".into(), json!(destination.group_id));
    forwarded.args.insert("message_mode".into(), json!(mode));
    let empty_to = forwarded
        .args
        .get("to")
        .is_none_or(|value| value.as_array().is_some_and(|items| items.is_empty()));
    if empty_to {
        forwarded.args.insert("to".into(), json!(recipients));
    }
    if let Some(key) = forwarded.args.remove("idempotency_key") {
        forwarded.args.entry("client_id").or_insert(key);
    }
    forwarded
        .args
        .entry("client_id")
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    forwarded.args.insert(
        "connect_reply_to".into(),
        json!(ConnectReplyReference {
            delivery_id: original.delivery_id.clone(),
            event_id: remote_event
        }),
    );
    let mut result = send_with_reply(
        home,
        &forwarded,
        Some(ReplyContext {
            original,
            local_event: target.clone(),
            target: destination,
        }),
        preflight,
    )?;
    if let Some(event) = result.get("source_event").cloned() {
        result.insert("event".into(), event);
    }
    Ok(result)
}

/// Called under the source Group permit, before the active record is removed.
/// A final receipt and its optional notification are independently recoverable.
pub(crate) fn notify_delivery_failure(
    home: &HomeLayout,
    entry: &ConnectOutboxEntry,
    receipt: &Event,
) -> Result<(), OpError> {
    let state = receipt
        .data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(state, "failed" | "unconfirmed") {
        return Ok(());
    }
    let work = &entry.work;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store
        .ledger_path(&work.source().group_id)
        .map_err(OpError::io)?;
    let client_id = format!("connect:failure:{}", work.delivery_id());
    if ledger::find_idempotent(&path, "system.notify", "system", &client_id)
        .map_err(OpError::io)?
        .is_some()
    {
        return Ok(());
    }
    let group = store.load(&work.source().group_id).map_err(OpError::io)?;
    let actor = cccc_core::actors::visible(&group).find(|actor| {
        actor.id == work.sender().id
            && connect_messages::actor_generation(actor) == work.sender().generation
    });
    let target = actor.map_or("user", |actor| actor.id.as_str());
    let cancellation = matches!(work, ConnectWork::Cancel(_));
    let title = match (cancellation, state) {
        (true, "failed") => "Connect reply cancellation failed",
        (true, _) => "Connect reply cancellation unconfirmed",
        (false, "failed") => "Connect delivery failed",
        (false, _) => "Connect delivery unconfirmed",
    };
    let next = if cancellation {
        "The reply request may still be active on the target. This does not stop an Actor or retract delivered work."
    } else if state == "failed" {
        "The target did not accept this message. Resolve the failure before sending a new message."
    } else {
        "The target may have received this message. Check with the target before sending again."
    };
    let reason = receipt
        .data
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("No delivery receipt")
        .chars()
        .take(1024)
        .collect::<String>();
    let text = format!(
        "{title}: {} / {}. {next} Reason: {reason}. Source event: {}.",
        work.target().instance_id,
        work.target().title,
        work.source_event_id()
    );
    super::messaging::send(home,&DaemonRequest { v:1,op:"system_notify".into(),args:json!({
        "group_id":group.group_id,"by":"system","client_id":client_id,"kind":"error",
        "title":title,"message":text,"to":[target],"target_actor_id":target,
        "im_visibility":"internal","related_event_id":entry.source_event.data.get("source_event_id").and_then(Value::as_str).unwrap_or(work.source_event_id()),
        "context":{"transport":"connect","delivery_id":work.delivery_id(),"state":state,"dst_instance_id":work.target().instance_id,"dst_group_id":work.target().group_id}
    }).as_object().expect("notification").clone() },"system.notify")?;
    Ok(())
}

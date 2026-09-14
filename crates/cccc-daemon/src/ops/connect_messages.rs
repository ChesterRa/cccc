//! Canonical peer messages and receipts share the ordinary Group ledger.
use crate::dispatch::{OpError, OpResult, object};
use base64::Engine;
use cccc_contracts::{DaemonRequest, Event, connect::ConnectPeerRequest, connect_message::*};
use cccc_core::{
    GroupStore, HomeLayout, actors, connect_delivery, connect_peer::PeerScope, ledger,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn receipt(
    home: &HomeLayout,
    envelope: &ConnectPeerRequest,
    scope: &PeerScope,
) -> OpResult {
    let cccc_contracts::connect::ConnectPeerOperation::Receipt {
        source_group_id,
        target_group_id,
        delivery_id,
        message_sha256,
        connection_id,
    } = &envelope.operation
    else {
        return Err(OpError::new("invalid_connect_request", "receipt required"));
    };
    check_scope(scope, source_group_id, target_group_id)?;
    if uuid::Uuid::parse_str(delivery_id).is_err() {
        return Err(OpError::new(
            "invalid_connect_request",
            "invalid delivery identity",
        ));
    }
    // A deleted Group cannot attest that an uncertain past delivery never existed.
    GroupStore::new(home.clone())
        .and_then(|store| store.load(target_group_id))
        .map_err(OpError::not_found)?;
    let event = find_received(
        home,
        &envelope.proof.source_instance_id,
        target_group_id,
        delivery_id,
    )?;
    let receipt = event
        .map(|event| {
            let original = stored_message(&event)?;
            if original.connection_id != *connection_id
                || original.source.group_id != *source_group_id
                || original.source.device_id != envelope.proof.source_device_id
                || original.target.device_id != envelope.proof.target_device_id
            {
                return Err(OpError::new(
                    "connect_scope_denied",
                    "receipt belongs to a different binding",
                ));
            }
            delivery_receipt(&event, message_sha256)
        })
        .transpose()?;
    object(json!({"receipt":receipt}))
}

pub(super) fn deliver(
    home: &HomeLayout,
    request: &DaemonRequest,
    envelope: &ConnectPeerRequest,
    scope: &PeerScope,
    message: &ConnectMessage,
    blobs: &[ConnectBlobPayload],
) -> OpResult {
    // The dispatcher acquired this exact target Group's write permit before entry.
    if request.args.get("group_id").and_then(Value::as_str) != Some(&message.target.group_id) {
        return Err(OpError::new(
            "invalid_connect_request",
            "target Group must match dispatcher scope",
        ));
    }
    check_scope(scope, &message.source.group_id, &message.target.group_id)?;
    connect_delivery::validate(message).map_err(OpError::invalid)?;
    let proof = &envelope.proof;
    if message.account_origin != proof.account_origin
        || message.account_id != proof.account_id
        || message.source.instance_id != proof.source_instance_id
        || message.source.device_id != proof.source_device_id
        || message.target.instance_id != proof.target_instance_id
        || message.target.device_id != proof.target_device_id
    {
        return Err(OpError::new(
            "connect_scope_denied",
            "message belongs to a different binding",
        ));
    }
    let digest = connect_delivery::digest(message);
    // Do this before expiry/generation/blob checks: an earlier committed delivery
    // remains the same delivery even if its response was lost and actors changed.
    if let Some(event) = find_received(
        home,
        &message.source.instance_id,
        &message.target.group_id,
        &message.delivery_id,
    )? {
        return object(json!({"receipt":delivery_receipt(&event, &digest)?,"duplicate":true}));
    }
    if chrono::DateTime::parse_from_rfc3339(&message.deliver_before).map_err(OpError::invalid)?
        <= chrono::Utc::now()
    {
        return Err(OpError::new(
            "connect_delivery_expired",
            "delivery window has ended",
        ));
    }
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&message.target.group_id))
        .map_err(OpError::not_found)?;
    for recipient in &message.recipients {
        if recipient.id == "user" {
            continue;
        }
        let current = actors::visible(&group).find(|actor| actor.id == recipient.id);
        if current.is_none_or(|actor| actor_generation(actor) != recipient.generation) {
            return Err(OpError::new(
                "connect_recipient_changed",
                "the original recipient no longer exists; refresh the directory and send a new message",
            ));
        }
    }
    validate_reply(home, message)?;
    let attachments = receive_blobs(home, message, blobs)?;
    let sender_name = if message.sender.title.trim().is_empty() {
        &message.sender.id
    } else {
        &message.sender.title
    };
    let mut args = json!({
        "group_id":group.group_id, "by":remote_sender(&message.source.instance_id),
        "text":message.text, "format":message.format, "insight":message.insight,
        "message_mode":message.message_mode, "to":message.recipients.iter().map(|actor|&actor.id).collect::<Vec<_>>(),
        "attachments":attachments, "client_id":received_client_id(&message.delivery_id),
        "sender_title":sender_name, "source_platform":"cccc_connect",
        "source_user_id":message.sender.id, "source_user_name":sender_name,
        "src_group_id":message.source.group_id, "src_group_title":message.source.title,
        "src_event_id":message.source_event_id, "src_instance_id":message.source.instance_id,
        "connect_message":message, "connect_message_sha256":digest,
    }).as_object().expect("object").clone();
    // Presentation metadata comes from our account directory, not an asserted
    // peer name. Missing metadata never changes routing or delivery acceptance.
    if let Ok(binding) = cccc_core::connect_peer::scoped_binding(
        home,
        &message.source.instance_id,
        message.connection_id.as_deref(),
    ) && binding.remote.device_id == message.source.device_id
    {
        args.insert(
            "src_instance_name".into(),
            json!(binding.remote.display_name),
        );
    }
    if let Some(reply) = &message.reply_to {
        args.insert("reply_to".into(), json!(reply.event_id));
    }
    let result = super::messaging::send(
        home,
        &DaemonRequest {
            v: 1,
            op: "send".into(),
            args,
        },
        "chat.message",
    )?;
    let event: Event = serde_json::from_value(result["event"].clone()).map_err(OpError::invalid)?;
    object(json!({"receipt":delivery_receipt(&event, &digest)?}))
}

pub(super) fn actor_generation(actor: &cccc_contracts::Actor) -> String {
    if actor.generation.is_empty() {
        format!("legacy:{}", actor.created_at)
    } else {
        actor.generation.clone()
    }
}

fn check_scope(scope: &PeerScope, source: &str, target: &str) -> Result<(), OpError> {
    if source.is_empty() || target.is_empty() || !scope.allows(source, Some(target)) {
        return Err(OpError::new(
            "connect_scope_denied",
            "peer cannot access this Group pair",
        ));
    }
    Ok(())
}

fn remote_sender(instance: &str) -> String {
    format!("connect:{instance}")
}
fn received_client_id(delivery: &str) -> String {
    format!("connect:received:{delivery}")
}

fn find_received(
    home: &HomeLayout,
    source: &str,
    target: &str,
    delivery: &str,
) -> Result<Option<Event>, OpError> {
    let path = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(target))
        .map_err(OpError::io)?;
    ledger::find_idempotent(
        &path,
        "chat.message",
        &remote_sender(source),
        &received_client_id(delivery),
    )
    .map_err(OpError::io)
}

pub(super) fn stored_message(event: &Event) -> Result<ConnectMessage, OpError> {
    serde_json::from_value(
        event
            .data
            .get("connect_message")
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(|_| {
        OpError::new(
            "invalid_connect_message",
            "original Connect message is unavailable",
        )
    })
}

fn delivery_receipt(
    event: &Event,
    expected_digest: &str,
) -> Result<ConnectDeliveryReceipt, OpError> {
    let original = stored_message(event)?;
    if connect_delivery::digest(&original) != expected_digest {
        return Err(OpError::new(
            "connect_delivery_conflict",
            "delivery ID already exists with different content",
        ));
    }
    Ok(ConnectDeliveryReceipt {
        delivery_id: original.delivery_id,
        message_sha256: expected_digest.into(),
        event_id: event.id.clone(),
        delivered_at: event.ts.clone(),
    })
}

fn validate_reply(home: &HomeLayout, message: &ConnectMessage) -> Result<(), OpError> {
    let Some(reply) = &message.reply_to else {
        return Ok(());
    };
    let event = super::messaging::find_event(home, &message.target.group_id, &reply.event_id)?;
    let original = stored_message(&event)?;
    connect_delivery::validate_reply(&original, message)
        .map_err(|message| OpError::new("connect_reply_denied", message))
}

fn receive_blobs(
    home: &HomeLayout,
    message: &ConnectMessage,
    blobs: &[ConnectBlobPayload],
) -> Result<Vec<Value>, OpError> {
    let invalid = || {
        OpError::new(
            "invalid_connect_attachment",
            "attachment content does not match the signed message",
        )
    };
    let expected: std::collections::HashSet<_> = message
        .attachments
        .iter()
        .map(|file| file.sha256.as_str())
        .collect();
    if blobs.len() != expected.len() {
        return Err(invalid());
    }
    let mut decoded = std::collections::HashMap::new();
    for blob in blobs {
        if !expected.contains(blob.sha256.as_str())
            || decoded.contains_key(&blob.sha256)
            || blob.base64.len() as u64 > connect_delivery::MAX_ATTACHMENT_BYTES.div_ceil(3) * 4
        {
            return Err(invalid());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&blob.base64)
            .map_err(|_| invalid())?;
        if format!("{:x}", Sha256::digest(&bytes)) != blob.sha256 {
            return Err(invalid());
        }
        decoded.insert(blob.sha256.clone(), bytes);
    }
    for file in &message.attachments {
        if decoded.get(&file.sha256).map(|bytes| bytes.len() as u64) != Some(file.bytes) {
            return Err(invalid());
        }
    }
    // All parts are verified before writing any target blob or message.
    for bytes in decoded.values() {
        cccc_core::blobs::store(home, &message.target.group_id, bytes).map_err(OpError::io)?;
    }
    Ok(message.attachments.iter().map(|file|json!({
        "path":format!("state/blobs/{}",file.sha256), "sha256":file.sha256,"bytes":file.bytes,
        "title":file.name,"mime_type":file.mime_type,"kind":if file.mime_type.starts_with("image/"){"image"}else{"file"},
    })).collect())
}

#[cfg(test)]
#[path = "connect_messages_tests.rs"]
mod tests;

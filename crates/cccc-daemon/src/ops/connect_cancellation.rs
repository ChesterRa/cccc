//! Reply-obligation cancellation shares message persistence and peer authority.
use super::{connect_messages, connect_outbound, connect_peer};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::{
    DaemonRequest, Event, connect::ConnectActor, connect::ConnectPeerRequest, connect_message::*,
};
use cccc_core::{
    GroupDoc, GroupStore, HomeLayout, connect_delivery, connect_peer::PeerScope, ledger,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn accept(
    home: &HomeLayout,
    request: &DaemonRequest,
    group: &GroupDoc,
    source: &Event,
    by: &str,
) -> OpResult {
    connect_peer::authorize_local_source(home, request)?;
    let original = connect_messages::stored_message(source)?;
    let incoming = source
        .data
        .get("src_instance_id")
        .and_then(Value::as_str)
        .is_some();
    let (local, target) = if incoming {
        (&original.target, &original.source)
    } else {
        (&original.source, &original.target)
    };
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let key = format!("connect:cancel:{}", original.delivery_id);
    // Either a sender or a Group user may repeat the same cancellation intent.
    let existing = ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .find(|event| {
            event.kind == "chat.reply_request.cancelled"
                && event.data.get("source_event_id").and_then(Value::as_str) == Some(&source.id)
        });
    if let Some(event) = existing {
        return object(json!({"event":event,"duplicate":true}));
    }
    let binding = cccc_core::connect_peer::scoped_binding(
        home,
        &target.instance_id,
        original.connection_id.as_deref(),
    )
    .map_err(|e| OpError::new("connect_peer_unavailable", e))?;
    if binding.local.device_id != local.device_id || binding.remote.device_id != target.device_id {
        return Err(OpError::new(
            "connect_cancel_denied",
            "original membership binding changed",
        ));
    }
    let sender = if by == "user" {
        ConnectActor {
            id: by.into(),
            title: "User".into(),
            generation: String::new(),
            enabled: true,
            role: None,
        }
    } else {
        let actor = cccc_core::actors::find(group, by)
            .ok_or_else(|| OpError::new("actor_not_found", "original sender no longer exists"))?;
        ConnectActor {
            id: by.into(),
            title: actor.title.clone(),
            generation: connect_messages::actor_generation(actor),
            enabled: actor.enabled,
            role: cccc_core::actors::effective_role(group, by),
        }
    };
    let digest = Sha256::digest(
        serde_json::to_vec(&json!([
            "cccc.connect.cancel",
            local.instance_id,
            local.device_id,
            group.group_id,
            original.delivery_id
        ]))
        .map_err(OpError::invalid)?,
    );
    let id = uuid::Uuid::from_slice(&digest[..16])
        .map_err(OpError::invalid)?
        .to_string();
    if let Some(mut entry) =
        connect_delivery::load(home, &target.instance_id, &id).map_err(OpError::io)?
    {
        let projected = connect_outbound::project_source(home, &mut entry).is_ok();
        return object(
            json!({"event":entry.source_event,"accepted":true,"queued":true,"delivery_id":id,"source_projected":projected,"duplicate":true}),
        );
    }
    let mut event = Event::new("chat.reply_request.cancelled", &group.group_id);
    event.by = by.into();
    event.scope_key = group.active_scope_key.clone();
    let now = chrono::Utc::now();
    let cancel = ConnectCancellation {
        connection_id: original.connection_id.clone(),
        delivery_id: id.clone(),
        account_origin: binding.account_origin,
        account_id: binding.account_id,
        source: local.clone(),
        sender,
        source_event_id: event.id.clone(),
        target: target.clone(),
        original_delivery_id: original.delivery_id.clone(),
        original_message_sha256: connect_delivery::digest(&original),
        original_source_event_id: original.source_event_id.clone(),
        original_deliver_before: original.deliver_before.clone(),
        created_at: now.to_rfc3339(),
        deliver_before: (now + chrono::Duration::seconds(connect_delivery::DELIVERY_SECONDS))
            .to_rfc3339(),
    };
    connect_delivery::validate_cancellation_for(&original, &cancel)
        .map_err(|e| OpError::new("connect_cancel_denied", e))?;
    event.data = json!({"source_event_id":source.id,"client_id":key,"connect_cancel":cancel})
        .as_object()
        .expect("event")
        .clone();
    let mut entry = connect_delivery::reserve(
        home,
        &ConnectOutboxEntry {
            work: ConnectWork::Cancel(Box::new(cancel)),
            source_event: event,
            progress: Default::default(),
        },
    )
    .map_err(OpError::io)?;
    let projected = connect_outbound::project_source(home, &mut entry).is_ok();
    object(
        json!({"event":entry.source_event,"accepted":true,"queued":true,"delivery_id":id,"source_projected":projected}),
    )
}

pub(super) fn receive(
    home: &HomeLayout,
    envelope: &ConnectPeerRequest,
    scope: &PeerScope,
    cancel: &ConnectCancellation,
) -> OpResult {
    connect_delivery::validate_cancellation(cancel).map_err(OpError::invalid)?;
    let proof = &envelope.proof;
    if !scope.allows(&cancel.source.group_id, Some(&cancel.target.group_id))
        || cancel.account_origin != proof.account_origin
        || cancel.account_id != proof.account_id
        || cancel.source.instance_id != proof.source_instance_id
        || cancel.source.device_id != proof.source_device_id
        || cancel.target.instance_id != proof.target_instance_id
        || cancel.target.device_id != proof.target_device_id
    {
        return Err(OpError::new(
            "connect_scope_denied",
            "cancellation belongs to another binding or Group pair",
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store
        .load(&cancel.target.group_id)
        .map_err(OpError::not_found)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    let by = format!("connect:{}", cancel.source.instance_id);
    let key = format!("connect:cancel_received:{}", cancel.delivery_id);
    let digest = connect_delivery::cancellation_digest(cancel);
    if let Some(event) = ledger::find_idempotent(&path, "chat.reply_request.cancelled", &by, &key)
        .map_err(OpError::io)?
    {
        if event
            .data
            .get("connect_cancel_sha256")
            .and_then(Value::as_str)
            != Some(&digest)
        {
            return Err(OpError::new(
                "connect_delivery_conflict",
                "cancellation ID has different content",
            ));
        }
        return receipt(&event, cancel, &digest);
    }
    if chrono::DateTime::parse_from_rfc3339(&cancel.deliver_before).map_err(OpError::invalid)?
        <= chrono::Utc::now()
    {
        return Err(OpError::new(
            "connect_delivery_expired",
            "cancellation window ended",
        ));
    }
    let original =
        match ledger::find_event(&path, &cancel.original_source_event_id).map_err(OpError::io)? {
            Some(event) => Some(event),
            None => ledger::find_idempotent(
                &path,
                "chat.message",
                &by,
                &format!("connect:received:{}", cancel.original_delivery_id),
            )
            .map_err(OpError::io)?,
        };
    if let Some(event) = &original {
        connect_delivery::validate_cancellation_for(
            &connect_messages::stored_message(event)?,
            cancel,
        )
        .map_err(|e| OpError::new("connect_cancel_denied", e))?;
    } else if chrono::DateTime::parse_from_rfc3339(&cancel.original_deliver_before)
        .map_err(OpError::invalid)?
        > chrono::Utc::now()
    {
        return Err(OpError::new(
            "connect_original_pending",
            "the original request may still be in transit",
        ));
    }
    let mut event = Event::new("chat.reply_request.cancelled", &group.group_id);
    event.by = by;
    event.scope_key = group.active_scope_key;
    event.data=json!({"source_event_id":original.as_ref().map(|event|&event.id),"client_id":key,"connect_cancel":cancel,"connect_cancel_sha256":digest,"not_delivered":original.is_none()}).as_object().expect("event").clone();
    ledger::append(&path, &event).map_err(OpError::io)?;
    receipt(&event, cancel, &digest)
}

fn receipt(event: &Event, cancel: &ConnectCancellation, digest: &str) -> OpResult {
    object(
        json!({"receipt":ConnectDeliveryReceipt { delivery_id:cancel.delivery_id.clone(),message_sha256:digest.into(),event_id:event.id.clone(),delivered_at:event.ts.clone() }}),
    )
}

#[cfg(test)]
#[path = "connect_cancellation_tests.rs"]
mod tests;

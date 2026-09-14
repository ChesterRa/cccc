use super::*;
use cccc_contracts::{Event, connect::ConnectActor, connect_message::*};

fn entry(homes: &[HomeLayout]) -> ConnectOutboxEntry {
    let peer = crate::instance_identity::InstanceIdentity::load(&homes[1]).expect("key");
    let binding = connect_peer::binding(&homes[0], &peer.peer_id).expect("binding");
    let mut event = Event::new("chat.message", "source-group");
    event.by = "user".into();
    let now = Utc::now();
    ConnectOutboxEntry {
        work: ConnectWork::Message(Box::new(ConnectMessage {
            connection_id: None,
            delivery_id: uuid::Uuid::new_v4().to_string(),
            account_origin: binding.account_origin,
            account_id: binding.account_id,
            source: ConnectGroupAddress {
                instance_id: binding.local.instance_id,
                device_id: binding.local.device_id,
                group_id: event.group_id.clone(),
                title: "Source".into(),
            },
            target: ConnectGroupAddress {
                instance_id: binding.remote.instance_id,
                device_id: binding.remote.device_id,
                group_id: "target-group".into(),
                title: "Target".into(),
            },
            sender: ConnectActor {
                id: "user".into(),
                title: "User".into(),
                generation: String::new(),
                enabled: true,
                role: None,
            },
            recipients: vec![ConnectActor {
                id: "worker".into(),
                title: "Worker".into(),
                generation: "generation-1".into(),
                enabled: true,
                role: None,
            }],
            source_event_id: event.id.clone(),
            text: "durable message".into(),
            format: "plain".into(),
            insight: None,
            message_mode: "send".into(),
            attachments: vec![],
            reply_to: None,
            created_at: now.to_rfc3339(),
            deliver_before: (now + chrono::Duration::seconds(DELIVERY_SECONDS)).to_rfc3339(),
            reply_before: (now + chrono::Duration::seconds(REPLY_SECONDS)).to_rfc3339(),
        })),
        source_event: event,
        progress: ConnectDeliveryProgress::default(),
    }
}

#[test]
fn durable_acceptance_reopens_and_conflicting_retries_do_not_change_content() {
    let (_temp, homes, _) = connect_peer::tests::fixture();
    let entry = entry(&homes);
    let home = &homes[0];
    reserve(home, &entry).expect("accept");
    let reopened = HomeLayout::from_path(home.root()).expect("reopen");
    let peer = &message(&entry).target.instance_id;
    let id = &message(&entry).delivery_id;
    assert_eq!(
        load(&reopened, peer, id).expect("load"),
        Some(entry.clone())
    );
    let mut progress = entry.progress.clone();
    progress.attempts = 1;
    progress.needs_receipt = true;
    update_progress(home, peer, id, progress.clone()).expect("checkpoint");
    assert_eq!(reserve(home, &entry).expect("retry").progress, progress);
    let mut changed = entry.clone();
    message_mut(&mut changed).text.push_str("changed");
    assert_eq!(
        reserve(home, &changed).expect_err("conflict").kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        load(home, peer, id).expect("load").expect("entry").work,
        entry.work
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path(home, peer, id).expect("path"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    remove_finalized(home, peer, id).expect("remove after ledger finalization");
    remove_finalized(home, peer, id).expect("repeat removal");
    assert!(pending_ids(home).expect("pending").is_empty());
}

#[test]
fn admission_bounds_pending_work_and_rejects_previous_device_binding() {
    let (_temp, homes, _) = connect_peer::tests::fixture();
    let base = entry(&homes);
    for _ in 0..MAX_PENDING_PER_PEER {
        let mut entry = base.clone();
        message_mut(&mut entry).delivery_id = uuid::Uuid::new_v4().to_string();
        reserve(&homes[0], &entry).expect("capacity");
    }
    assert_eq!(
        reserve(&homes[0], &base).expect_err("full").kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(load(&homes[0], "../escape", &message(&base).delivery_id).is_err());
    crate::membership::update(&homes[0], |state| {
        state.device_id = Some("rebound".into());
        Ok(())
    })
    .expect("rebind");
    assert_eq!(
        reserve(&homes[0], &base).expect_err("old binding").kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn message_limits_include_logical_lifetimes_generations_and_combined_attachments() {
    let (_temp, homes, _) = connect_peer::tests::fixture();
    let entry = entry(&homes);
    let base = message(&entry).clone();
    validate(&base).expect("valid");
    let mut message = base.clone();
    message.recipients[0].generation.clear();
    assert!(validate(&message).is_err());
    let mut message = base.clone();
    message.deliver_before = message.reply_before.clone();
    assert!(validate(&message).is_err());
    let mut message = base;
    message.attachments = vec![
        ConnectAttachment {
            sha256: "a".repeat(64),
            bytes: MAX_ATTACHMENT_BYTES,
            name: "a.bin".into(),
            mime_type: "application/octet-stream".into()
        };
        2
    ];
    assert!(validate(&message).is_err());
}

fn message(entry: &ConnectOutboxEntry) -> &ConnectMessage {
    let ConnectWork::Message(message) = &entry.work else {
        panic!("message fixture")
    };
    message
}
fn message_mut(entry: &mut ConnectOutboxEntry) -> &mut ConnectMessage {
    let ConnectWork::Message(message) = &mut entry.work else {
        panic!("message fixture")
    };
    message
}

fn cancellation(original: &ConnectMessage) -> ConnectCancellation {
    let now = Utc::now();
    ConnectCancellation {
        connection_id: None,
        delivery_id: uuid::Uuid::new_v4().to_string(),
        account_origin: original.account_origin.clone(),
        account_id: original.account_id.clone(),
        source: original.source.clone(),
        sender: original.sender.clone(),
        source_event_id: uuid::Uuid::new_v4().to_string(),
        target: original.target.clone(),
        original_delivery_id: original.delivery_id.clone(),
        original_message_sha256: digest(original),
        original_source_event_id: original.source_event_id.clone(),
        original_deliver_before: original.deliver_before.clone(),
        created_at: now.to_rfc3339(),
        deliver_before: (now + chrono::Duration::seconds(DELIVERY_SECONDS)).to_rfc3339(),
    }
}

#[test]
fn cancellation_preserves_pair_generation_and_original_and_shares_capacity() {
    let (_temp, homes, _) = connect_peer::tests::fixture();
    let mut original = entry(&homes);
    message_mut(&mut original).message_mode = "request_reply".into();
    message_mut(&mut original).sender = message(&original).recipients[0].clone();
    original.source_event.by = "worker".into();
    let cancel = cancellation(message(&original));
    validate_cancellation(&cancel).expect("valid control");
    validate_cancellation_for(message(&original), &cancel).expect("sender");
    for mutate in [
        (|c: &mut ConnectCancellation| c.sender.generation.push_str("-replacement"))
            as fn(&mut ConnectCancellation),
        |c| c.target.group_id.push_str("-other"),
        |c| c.source.device_id.push_str("-replacement"),
        |c| c.original_message_sha256 = "0".repeat(64),
        |c| c.original_source_event_id = uuid::Uuid::new_v4().to_string(),
        |c| c.account_id.push_str("-other"),
    ] {
        let mut changed = cancel.clone();
        mutate(&mut changed);
        assert!(validate_cancellation_for(message(&original), &changed).is_err());
    }
    let mut reverse = cancel.clone();
    std::mem::swap(&mut reverse.source, &mut reverse.target);
    assert!(
        validate_cancellation_for(message(&original), &reverse).is_err(),
        "recipient Actor cannot cancel"
    );
    reverse.sender.id = "user".into();
    reverse.sender.generation.clear();
    validate_cancellation_for(message(&original), &reverse).expect("participating human");
    let mut event = Event::new("chat.reply_request.cancelled", &cancel.source.group_id);
    event.id = cancel.source_event_id.clone();
    event.by = cancel.sender.id.clone();
    let control = ConnectOutboxEntry {
        work: ConnectWork::Cancel(Box::new(cancel)),
        source_event: event,
        progress: Default::default(),
    };
    let mut mismatched = control.clone();
    mismatched.source_event.kind = "chat.message".into();
    assert!(
        reserve(&homes[0], &mismatched).is_err(),
        "projection type must agree"
    );
    reserve(&homes[0], &control).expect("control accepted");
    for _ in 1..MAX_PENDING_PER_PEER {
        let mut another = original.clone();
        message_mut(&mut another).delivery_id = uuid::Uuid::new_v4().to_string();
        reserve(&homes[0], &another).expect("shared capacity");
    }
    assert_eq!(
        reserve(&homes[0], &original)
            .expect_err("both kinds count")
            .kind(),
        io::ErrorKind::WouldBlock
    );
}

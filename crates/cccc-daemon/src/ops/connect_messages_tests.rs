use super::*;
use cccc_contracts::connect::{ConnectActor, ConnectPeerOperation};

#[tokio::test]
async fn group_pair_delivery_and_receipt_cannot_expand_scope_or_change_an_accepted_body() {
    let (_temp, state, peer, server) = crate::connect_transport::tests::setup().await;
    let source = GroupStore::new(state.source.clone())
        .expect("store")
        .create("Source", "")
        .expect("group");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let target = target_store.create("Shared target", "").expect("group");
    let other = target_store.create("Private target", "").expect("group");
    let binding = cccc_core::connect_peer::binding(&state.source, &peer).expect("binding");
    let now = chrono::Utc::now();
    let user = ConnectActor {
        id: "user".into(),
        title: "User".into(),
        generation: String::new(),
        enabled: true,
        role: None,
    };
    let message = ConnectMessage {
        connection_id: None,
        delivery_id: uuid::Uuid::new_v4().to_string(),
        source_event_id: uuid::Uuid::new_v4().to_string(),
        account_origin: binding.account_origin,
        account_id: binding.account_id,
        source: ConnectGroupAddress {
            instance_id: binding.local.instance_id,
            device_id: binding.local.device_id,
            group_id: source.group_id.clone(),
            title: source.title,
        },
        target: ConnectGroupAddress {
            instance_id: peer.clone(),
            device_id: binding.remote.device_id,
            group_id: target.group_id.clone(),
            title: target.title,
        },
        sender: user.clone(),
        recipients: vec![user],
        text: "shared message".into(),
        format: "plain".into(),
        insight: None,
        message_mode: "send".into(),
        attachments: vec![],
        reply_to: None,
        created_at: now.to_rfc3339(),
        deliver_before: (now + chrono::Duration::seconds(60)).to_rfc3339(),
        reply_before: (now + chrono::Duration::days(1)).to_rfc3339(),
    };
    let scope = PeerScope::GroupPair {
        source_group_id: source.group_id.clone(),
        target_group_id: target.group_id.clone(),
    };
    let envelope = cccc_core::connect_peer::sign_request(
        &state.source,
        &peer,
        ConnectPeerOperation::Deliver {
            message: Box::new(message.clone()),
            blobs: vec![],
        },
    )
    .expect("proof");
    let request = DaemonRequest {
        v: 1,
        op: "connect_peer_receive".into(),
        args: json!({"group_id":target.group_id,"envelope":envelope})
            .as_object()
            .expect("args")
            .clone(),
    };
    for rejected_scope in [
        PeerScope::GroupPair {
            source_group_id: "another-source".into(),
            target_group_id: target.group_id.clone(),
        },
        PeerScope::GroupPair {
            source_group_id: source.group_id.clone(),
            target_group_id: other.group_id.clone(),
        },
    ] {
        assert_eq!(
            deliver(
                &state.target,
                &request,
                &envelope,
                &rejected_scope,
                &message,
                &[]
            )
            .expect_err("scope denied")
            .code,
            "connect_scope_denied"
        );
    }
    assert!(
        ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events")
            .is_empty()
    );
    let accepted =
        deliver(&state.target, &request, &envelope, &scope, &message, &[]).expect("exact pair");
    let original_event = accepted["receipt"]["event_id"]
        .as_str()
        .expect("event")
        .to_owned();
    let recorded = ledger::find_event(
        &target_store.ledger_path(&target.group_id).expect("ledger"),
        &original_event,
    )
    .expect("read")
    .expect("message");
    assert_eq!(recorded.data["src_instance_name"], "0");
    assert_eq!(recorded.data["source_user_id"], message.sender.id);
    let duplicate =
        deliver(&state.target, &request, &envelope, &scope, &message, &[]).expect("same delivery");
    assert_eq!(duplicate["receipt"]["event_id"], original_event);
    let mut changed = message.clone();
    changed.text = "changed after acceptance".into();
    let changed_envelope = cccc_core::connect_peer::sign_request(
        &state.source,
        &peer,
        ConnectPeerOperation::Deliver {
            message: Box::new(changed.clone()),
            blobs: vec![],
        },
    )
    .expect("new wire proof");
    assert_eq!(
        deliver(
            &state.target,
            &request,
            &changed_envelope,
            &scope,
            &changed,
            &[]
        )
        .expect_err("logical identity conflict")
        .code,
        "connect_delivery_conflict"
    );
    let query = cccc_core::connect_peer::sign_request(
        &state.source,
        &peer,
        ConnectPeerOperation::Receipt {
            connection_id: None,
            source_group_id: source.group_id.clone(),
            target_group_id: target.group_id.clone(),
            delivery_id: message.delivery_id.clone(),
            message_sha256: connect_delivery::digest(&message),
        },
    )
    .expect("receipt query");
    assert_eq!(
        receipt(&state.target, &query, &scope).expect("receipt")["receipt"]["event_id"],
        original_event
    );
    assert!(
        receipt(
            &state.target,
            &query,
            &PeerScope::GroupPair {
                source_group_id: source.group_id,
                target_group_id: other.group_id.clone()
            }
        )
        .is_err()
    );
    assert!(
        ledger::read_all(&target_store.ledger_path(&other.group_id).expect("path"))
            .expect("events")
            .is_empty()
    );
    let mut wrong_lock = request;
    wrong_lock
        .args
        .insert("group_id".into(), json!(other.group_id));
    assert!(
        !crate::dispatch::dispatch(&state.target, &wrong_lock).ok,
        "transport cannot name a different lock scope"
    );
    target_store
        .delete(&target.group_id)
        .expect("delete shared fixture Group");
    assert!(
        receipt(&state.target, &query, &scope).is_err(),
        "deleted history cannot prove an uncertain delivery was absent"
    );
    server.abort();
}

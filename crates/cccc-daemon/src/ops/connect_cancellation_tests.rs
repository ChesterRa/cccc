use super::*;
use cccc_contracts::connect::ConnectPeerOperation;

#[tokio::test]
async fn connect_cancellation_scope_deadline_and_missing_original_are_bounded() {
    let (_temp, state, peer, server) = crate::connect_transport::tests::setup().await;
    let source = GroupStore::new(state.source.clone())
        .expect("store")
        .create("Source", "")
        .expect("group");
    let store = GroupStore::new(state.target.clone()).expect("store");
    let target = store.create("Target", "").expect("group");
    let private = store.create("Private", "").expect("group");
    let binding = cccc_core::connect_peer::binding(&state.source, &peer).expect("binding");
    let now = chrono::Utc::now();
    let cancel = ConnectCancellation {
        connection_id: None,
        delivery_id: uuid::Uuid::new_v4().to_string(),
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
        sender: ConnectActor {
            id: "user".into(),
            title: "User".into(),
            generation: String::new(),
            enabled: true,
            role: None,
        },
        source_event_id: uuid::Uuid::new_v4().to_string(),
        original_delivery_id: uuid::Uuid::new_v4().to_string(),
        original_message_sha256: "0".repeat(64),
        original_source_event_id: uuid::Uuid::new_v4().to_string(),
        original_deliver_before: (now + chrono::Duration::seconds(60)).to_rfc3339(),
        created_at: now.to_rfc3339(),
        deliver_before: (now + chrono::Duration::seconds(120)).to_rfc3339(),
    };
    let proof = |cancel: &ConnectCancellation| {
        cccc_core::connect_peer::sign_request(
            &state.source,
            &peer,
            ConnectPeerOperation::Cancel {
                cancellation: Box::new(cancel.clone()),
            },
        )
        .expect("proof")
    };
    let scope = PeerScope::GroupPair {
        source_group_id: source.group_id.clone(),
        target_group_id: target.group_id.clone(),
    };
    assert_eq!(
        receive(&state.target, &proof(&cancel), &scope, &cancel)
            .expect_err("original still deliverable")
            .code,
        "connect_original_pending"
    );
    for rejected in [
        PeerScope::GroupPair {
            source_group_id: "other".into(),
            target_group_id: target.group_id.clone(),
        },
        PeerScope::GroupPair {
            source_group_id: source.group_id.clone(),
            target_group_id: private.group_id.clone(),
        },
    ] {
        assert_eq!(
            receive(&state.target, &proof(&cancel), &rejected, &cancel)
                .expect_err("other group")
                .code,
            "connect_scope_denied"
        );
    }
    let mut changed = cancel.clone();
    changed.target.device_id.push_str("replacement");
    assert_eq!(
        receive(&state.target, &proof(&changed), &scope, &changed)
            .expect_err("previous binding")
            .code,
        "connect_scope_denied"
    );
    let mut expired = cancel.clone();
    expired.created_at = (now - chrono::Duration::seconds(120)).to_rfc3339();
    expired.deliver_before = (now - chrono::Duration::seconds(1)).to_rfc3339();
    assert_eq!(
        receive(&state.target, &proof(&expired), &scope, &expired)
            .expect_err("expired new control")
            .code,
        "connect_delivery_expired"
    );
    // An original that never arrived cannot acquire an obligation after its delivery window.
    let mut settled = cancel.clone();
    settled.original_deliver_before = (now - chrono::Duration::seconds(1)).to_rfc3339();
    let envelope = proof(&settled);
    let accepted = receive(&state.target, &envelope, &scope, &settled).expect("settled absence");
    assert_eq!(
        receive(&state.target, &envelope, &scope, &settled).expect("duplicate"),
        accepted
    );
    let mut conflict = settled.clone();
    conflict.original_message_sha256 = "1".repeat(64);
    assert_eq!(
        receive(&state.target, &proof(&conflict), &scope, &conflict)
            .expect_err("ID cannot change")
            .code,
        "connect_delivery_conflict"
    );
    let events =
        ledger::read_all(&store.ledger_path(&target.group_id).expect("path")).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["not_delivered"], true);
    assert!(events[0].data["source_event_id"].is_null());
    assert!(
        ledger::read_all(&store.ledger_path(&private.group_id).expect("path"))
            .expect("private")
            .is_empty()
    );
    let request = DaemonRequest {
        v: 1,
        op: "connect_peer_receive".into(),
        args: json!({"group_id":private.group_id,"envelope":envelope})
            .as_object()
            .expect("args")
            .clone(),
    };
    assert!(
        !crate::dispatch::dispatch(&state.target, &request).ok,
        "wrong dispatcher lock scope"
    );
    server.abort();
}

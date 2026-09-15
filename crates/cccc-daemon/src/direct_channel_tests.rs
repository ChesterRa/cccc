use super::*;
use crate::connect_transport::{PeerClient, tests::send_request};
use cccc_core::{GroupStore, connect_catalog};
#[tokio::test]
async fn pending_direct_peer_recovers_approval_after_expiry() {
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let ga = GroupStore::new(a.clone())
        .expect("store")
        .create("A", "")
        .expect("group");
    let gb = GroupStore::new(b.clone())
        .expect("store")
        .create("B", "")
        .expect("group");
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let address = reserve.local_addr().expect("address").to_string();
    drop(reserve);
    direct::configure(
        &a,
        Some(DirectListener {
            bind: address.clone(),
            address,
        }),
        None,
    )
    .expect("listener");
    let text = direct::invite(&a, &ga.group_id).expect("invite");
    let id = direct::join(&b, &gb.group_id, &text).expect("join");
    let joining = direct::load(&b).expect("store").relations.remove(0);
    direct::hello(
        &a,
        &id,
        &joining.invitation.as_ref().expect("invitation").secret,
        &joining.local,
        &joining.local.public_key,
    )
    .expect("request received");
    direct::set_state(&a, &ga.group_id, &id, true).expect("approve while peer is offline");
    for home in [&a, &b] {
        direct::update(home, |s| {
            s.relations[0].expires_at = "2000-01-01T00:00:00Z".into();
            Ok(())
        })
        .expect("expiry boundary");
    }
    assert!(
        dial(
            b.clone(),
            DispatchLocks::default(),
            Channels::default(),
            joining
        )
        .await
        .is_err()
    );
    assert_eq!(
        direct::load(&b)
            .expect("transport failure retains pending")
            .relations[0]
            .state,
        DirectState::Pending
    );
    let ca = Channels::default();
    let cb = Channels::default();
    let mut services = JoinSet::new();
    services.spawn(run(a.clone(), DispatchLocks::default(), ca.clone()));
    wait(|| {
        cccc_core::fs::read_json::<Value>(&a.root().join("state/connect/direct_status.json"))
            .ok()
            .is_some_and(|s| s["listener"] == true)
    })
    .await;
    services.spawn(run(b.clone(), DispatchLocks::default(), cb.clone()));
    let recovered = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if direct::load(&b).expect("store").relations[0].state == DirectState::Active
                && ca.0.lock().await.contains_key(&id)
                && cb.0.lock().await.contains_key(&id)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await;
    if recovered.is_ok() {
        let peer = InstanceIdentity::load(&b).expect("peer identity").peer_id;
        crate::connect_transport::refresh_catalog_scoped(
            &a,
            &PeerClient {
                http: None,
                direct: ca,
            },
            &peer,
            Some(&id),
        )
        .await
        .expect("recovered channel can exchange an authorized catalog");
    }
    services.abort_all();
    while services.join_next().await.is_some() {}
    assert!(
        recovered.is_ok(),
        "receiver approval must survive expiry and a lost/offline response"
    );
}
async fn wait(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("condition converges");
}
fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    }
}
#[tokio::test]
async fn two_homes_pair_and_exchange_files_replies_reconnect_and_revoke_without_web_or_account() {
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let sa = GroupStore::new(a.clone()).expect("store");
    let sb = GroupStore::new(b.clone()).expect("store");
    let ga = sa.create("A", "").expect("group");
    let mut gb = sb.create("B", "").expect("group");
    cccc_core::actors::add(&mut gb, cccc_contracts::Actor::new("worker")).expect("actor");
    sb.save(&gb).expect("save");
    let unused = sb.create("Unshared", "").expect("group");
    let reserve = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let address = reserve.local_addr().expect("address").to_string();
    drop(reserve);
    direct::configure(
        &a,
        Some(DirectListener {
            bind: address.clone(),
            address,
        }),
        None,
    )
    .expect("configure");
    let text = direct::invite(&a, &ga.group_id).expect("invite");
    let id = direct::join(&b, &gb.group_id, &text).expect("join");
    let ka = InstanceIdentity::load(&a).expect("key");
    let kb = InstanceIdentity::load(&b).expect("key");
    assert!(
        !cccc_core::membership::load(&a)
            .expect("membership")
            .logged_in
    );
    assert!(direct::load(&b).expect("store").listener.is_none());
    let ca = Channels::default();
    let cb = Channels::default();
    let la = DispatchLocks::default();
    let lb = DispatchLocks::default();
    let mut services = JoinSet::new();
    services.spawn(run(a.clone(), la.clone(), ca.clone()));
    services.spawn(run(b.clone(), lb.clone(), cb.clone()));
    wait(|| direct::load(&a).expect("store").relations[0].state == DirectState::Pending).await;
    assert!(direct::binding(&a, &kb.peer_id, &id).is_err());
    direct::set_state(&a, &ga.group_id, &id, true).expect("approve");
    wait(|| direct::load(&b).expect("store").relations[0].state == DirectState::Active).await;
    let client_a = PeerClient {
        http: None,
        direct: ca.clone(),
    };
    let client_b = PeerClient {
        http: None,
        direct: cb.clone(),
    };
    crate::connect_transport::refresh_catalog_scoped(&a, &client_a, &kb.peer_id, Some(&id))
        .await
        .expect("catalog");
    let catalog = connect_catalog::load_scoped(&a, &kb.peer_id, Some(&id))
        .expect("cache")
        .expect("catalog");
    assert_eq!(catalog.groups.len(), 1);
    assert_eq!(catalog.groups[0].group_id, gb.group_id);
    assert!(
        !crate::dispatch::dispatch(
            &a,
            &send_request(
                &ga.group_id,
                &unused.group_id,
                &kb.peer_id,
                json!(["worker"]),
                "not-shared"
            )
        )
        .ok
    );
    let mut send = send_request(
        &ga.group_id,
        &gb.group_id,
        &kb.peer_id,
        json!(["worker"]),
        "direct-question",
    );
    send.args
        .insert("message_mode".into(), json!("request_reply"));
    let bytes = vec![42; 96 * 1024];
    let blob = cccc_core::blobs::store(&a, &ga.group_id, &bytes).expect("blob");
    send.args.insert(
        "attachments".into(),
        json!([{"path":blob.path,"name":"bytes.bin","mime_type":"application/octet-stream"}]),
    );
    let sent = crate::dispatch::dispatch(&a, &send);
    assert!(sent.ok, "{sent:?}");
    let delivery = sent.result["delivery_id"].as_str().expect("id");
    let mut interrupted = cccc_core::connect_delivery::load(&a, &kb.peer_id, delivery)
        .expect("outbox")
        .expect("pending");
    crate::connect_transport::delivery::process(&a, &client_a, &la, &kb.peer_id, delivery)
        .await
        .expect("deliver");
    let events =
        cccc_core::ledger::read_all(&sb.ledger_path(&gb.group_id).expect("path")).expect("ledger");
    let received = events
        .iter()
        .find(|e| e.kind == "chat.message")
        .expect("message");
    let blob = received.data["attachments"][0]["path"]
        .as_str()
        .expect("blob");
    assert_eq!(
        std::fs::read(cccc_core::blobs::resolve(&b, &gb.group_id, blob).expect("path"))
            .expect("bytes"),
        bytes
    );
    let reply = crate::dispatch::dispatch(
        &b,
        &request(
            "reply",
            json!({"group_id":gb.group_id,"by":"worker","reply_to":received.id,"text":"direct reply","client_id":"direct-reply"}),
        ),
    );
    assert!(reply.ok, "{reply:?}");
    // Stop both channel services; durable work and grants survive. The joining side still has no listener.
    services.abort_all();
    while services.join_next().await.is_some() {}
    let ca = Channels::default();
    let cb = Channels::default();
    services.spawn(run(a.clone(), la.clone(), ca.clone()));
    services.spawn(run(b.clone(), lb.clone(), cb.clone()));
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if ca.0.lock().await.contains_key(&id) && cb.0.lock().await.contains_key(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("reconnect");
    let client_b = PeerClient {
        direct: cb,
        ..client_b
    };
    crate::connect_transport::delivery::process(
        &b,
        &client_b,
        &lb,
        &ka.peer_id,
        reply.result["delivery_id"].as_str().expect("id"),
    )
    .await
    .expect("reverse delivery");
    let events =
        cccc_core::ledger::read_all(&sa.ledger_path(&ga.group_id).expect("path")).expect("ledger");
    assert!(
        events
            .iter()
            .any(|e| e.data.get("text") == Some(&json!("direct reply")))
    );
    // Simulate loss after remote acceptance but before the local receipt checkpoint.
    interrupted.progress.needs_receipt = true;
    cccc_core::connect_delivery::reserve(&a, &interrupted).expect("recover pending checkpoint");
    let recovered = PeerClient {
        http: client_a.http.clone(),
        direct: ca.clone(),
    };
    crate::connect_transport::delivery::process(&a, &recovered, &la, &kb.peer_id, delivery)
        .await
        .expect("recover receipt");
    let received_again =
        cccc_core::ledger::read_all(&sb.ledger_path(&gb.group_id).expect("path")).expect("ledger");
    assert_eq!(
        received_again
            .iter()
            .filter(|e| e.kind == "chat.message"
                && e.data.get("text") == Some(&json!("durable Connect message")))
            .count(),
        1
    );
    let queued = crate::dispatch::dispatch(
        &a,
        &send_request(
            &ga.group_id,
            &gb.group_id,
            &kb.peer_id,
            json!(["worker"]),
            "queued-before-revoke",
        ),
    );
    assert!(queued.ok, "{queued:?}");
    direct::set_state(&a, &ga.group_id, &id, false).expect("revoke");
    let client_a = PeerClient {
        direct: ca,
        ..client_a
    };
    assert!(
        crate::connect_transport::refresh_catalog_scoped(&a, &client_a, &kb.peer_id, Some(&id))
            .await
            .is_err()
    );
    assert!(
        !crate::dispatch::dispatch(
            &a,
            &send_request(
                &ga.group_id,
                &gb.group_id,
                &kb.peer_id,
                json!(["worker"]),
                "revoked"
            )
        )
        .ok
    );
    crate::connect_transport::delivery::process(
        &a,
        &client_a,
        &la,
        &kb.peer_id,
        queued.result["delivery_id"].as_str().expect("id"),
    )
    .await
    .expect("retire revoked pending work");
    assert!(
        cccc_core::connect_delivery::pending_ids(&a)
            .expect("pending")
            .is_empty()
    );
    services.abort_all();
}
#[tokio::test]
async fn malformed_or_oversized_frames_are_rejected_before_payload_allocation() {
    use tokio::io::AsyncWriteExt;
    let (mut writer, mut reader) = tokio::io::duplex(64);
    writer
        .write_u32((MAX_FRAME + 1) as u32)
        .await
        .expect("header");
    assert!(read::<Frame>(&mut reader, MAX_FRAME).await.is_err());
    writer.write_u32(1).await.expect("header");
    writer.write_all(b"{").await.expect("payload");
    assert!(read::<Frame>(&mut reader, MAX_FRAME).await.is_err());
}

#[tokio::test]
async fn standalone_background_catalog_and_outbox_need_no_account_snapshot() {
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let sa = GroupStore::new(a.clone()).expect("store");
    let sb = GroupStore::new(b.clone()).expect("store");
    let ga = sa.create("A", "").expect("group");
    let mut gb = sb.create("B", "").expect("group");
    cccc_core::actors::add(&mut gb, cccc_contracts::Actor::new("worker")).expect("actor");
    sb.save(&gb).expect("save");
    let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let address = socket.local_addr().expect("address").to_string();
    drop(socket);
    direct::configure(
        &a,
        Some(DirectListener {
            bind: address.clone(),
            address,
        }),
        Some("Receiving office"),
    )
    .expect("listener");
    let invitation = direct::invite(&a, &ga.group_id).expect("invitation");
    let id = direct::join(&b, &gb.group_id, &invitation).expect("join");
    let mut tasks = JoinSet::new();
    tasks.spawn(crate::connect_transport::run(
        a.clone(),
        DispatchLocks::default(),
    ));
    tasks.spawn(crate::connect_transport::run(
        b.clone(),
        DispatchLocks::default(),
    ));
    wait(|| direct::load(&a).expect("store").relations[0].state == DirectState::Pending).await;
    direct::set_state(&a, &ga.group_id, &id, true).expect("approve");
    let peer = InstanceIdentity::load(&b).expect("key").peer_id;
    wait(|| {
        connect_catalog::load_scoped(&a, &peer, Some(&id))
            .expect("catalog")
            .is_some()
    })
    .await;
    // A damaged optional account cache must not disable standalone discovery.
    std::fs::create_dir_all(a.root().join("secrets")).expect("secrets");
    std::fs::write(
        a.root().join("secrets/connect.json"),
        b"invalid account cache",
    )
    .expect("fixture");
    let discover = crate::dispatch::dispatch(
        &a,
        &request(
            "connect_catalog",
            json!({"group_id":ga.group_id,"by":"user"}),
        ),
    );
    assert!(discover.ok);
    assert_eq!(discover.result["external_groups"][0]["transport"], "direct");
    let accepted = crate::dispatch::dispatch(
        &a,
        &send_request(
            &ga.group_id,
            &gb.group_id,
            &peer,
            json!(["worker"]),
            "automatic-direct-delivery",
        ),
    );
    assert!(accepted.ok, "{accepted:?}");
    wait(|| {
        cccc_core::ledger::read_all(&sb.ledger_path(&gb.group_id).expect("path"))
            .expect("events")
            .iter()
            .any(|e| e.kind == "chat.message")
    })
    .await;
    wait(|| {
        cccc_core::connect_delivery::pending_ids(&a)
            .expect("outbox")
            .is_empty()
    })
    .await;
    tasks.abort_all();
}

#[tokio::test]
async fn pending_direct_peer_reconciles_expired_removed_and_deleted_receiver_grants() {
    for decision in ["expired", "removed", "deleted_group"] {
        let temp = tempfile::tempdir().expect("temp");
        let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
        let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
        let sa = GroupStore::new(a.clone()).expect("store");
        let ga = sa.create("A", "").expect("group");
        let gb = GroupStore::new(b.clone())
            .expect("store")
            .create("B", "")
            .expect("group");
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
        let address = reserve.local_addr().expect("address").to_string();
        drop(reserve);
        direct::configure(
            &a,
            Some(DirectListener {
                bind: address.clone(),
                address,
            }),
            None,
        )
        .expect("listener");
        let text = direct::invite(&a, &ga.group_id).expect("invite");
        let id = direct::join(&b, &gb.group_id, &text).expect("join");
        for home in [&a, &b] {
            direct::update(home, |s| {
                s.relations[0].expires_at = "2000-01-01T00:00:00Z".into();
                Ok(())
            })
            .expect("deadline passed");
        }
        match decision {
            "removed" => direct::remove(&a, &ga.group_id, &id).expect("remove expired invitation"),
            "deleted_group" => {
                sa.delete(&ga.group_id).expect("delete receiver Group");
            }
            _ => {}
        }
        let expected = if decision == "expired" {
            DirectState::Expired
        } else {
            DirectState::Revoked
        };
        let mut services = JoinSet::new();
        let ca = Channels::default();
        let cb = Channels::default();
        services.spawn(run(a.clone(), DispatchLocks::default(), ca.clone()));
        wait(|| {
            cccc_core::fs::read_json::<Value>(&a.root().join("state/connect/direct_status.json"))
                .ok()
                .is_some_and(|s| s["listener"] == true)
        })
        .await;
        services.spawn(run(b.clone(), DispatchLocks::default(), cb.clone()));
        wait(|| direct::load(&b).expect("store").relations[0].state == expected).await;
        assert!(ca.0.lock().await.is_empty());
        assert!(cb.0.lock().await.is_empty());
        assert!(
            direct::set_state(&a, &ga.group_id, &id, true).is_err(),
            "no new approval past expiry or retirement"
        );
        direct::remove(&b, &gb.group_id, &id).expect("remove confirmed closed record");
        assert!(
            direct::join(&b, &gb.group_id, &text).is_err(),
            "retired ID cannot be replayed"
        );
        services.abort_all();
        while services.join_next().await.is_some() {}
    }
}

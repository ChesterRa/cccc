use super::*;
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use base64::Engine;
use cccc_contracts::{
    DaemonRequest,
    connect::{ConnectDirectory, ConnectInstance, ConnectPeerRequest},
};
use cccc_core::{
    GroupStore, connect::ConnectSnapshot, instance_identity::InstanceIdentity, membership,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub(crate) fn client() -> PeerClient {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client")
        .into()
}

pub(crate) fn send_request(
    source: &str,
    target: &str,
    peer: &str,
    to: Value,
    client_id: &str,
) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: "connect_send".into(),
        args: json!({
            "group_id":source,"target_group_id":target,"instance_id":peer,"by":"user",
            "text":"durable Connect message","message_mode":"send","to":to,"client_id":client_id,
        })
        .as_object()
        .expect("args")
        .clone(),
    }
}

#[tokio::test]
async fn connect_delivery_recovers_lost_receipt_without_resending_and_keeps_original_recipients() {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("source store");
    let source = source_store.create("Source", "").expect("source");
    let target_store = GroupStore::new(state.target.clone()).expect("target store");
    let mut target = target_store.create("Target", "").expect("target");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("lead")).expect("lead");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("worker")).expect("worker");
    target_store.save(&target).expect("save");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let file = cccc_core::blobs::store(
        &state.source,
        &source.group_id,
        b"file travels with its original message",
    )
    .expect("blob");
    let mut request = send_request(
        &source.group_id,
        &target.group_id,
        &peer,
        json!(["@all"]),
        "repeat-me",
    );
    request.args.insert(
        "attachments".into(),
        json!([{"path":file.path,"title":"fixture.txt","mime_type":"text/plain"}]),
    );
    let accepted = crate::dispatch::dispatch(&state.source, &request);
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"]
        .as_str()
        .expect("delivery ID");
    let entry = cccc_core::connect_delivery::load(&state.source, &peer, id)
        .expect("load")
        .expect("queued");
    assert_eq!(
        message(&entry)
            .recipients
            .iter()
            .map(|actor| actor.id.as_str())
            .collect::<Vec<_>>(),
        ["lead", "worker"]
    );
    let repeated = crate::dispatch::dispatch(&state.source, &request);
    assert_eq!(
        repeated.result["delivery_id"],
        accepted.result["delivery_id"]
    );
    assert_eq!(repeated.result["duplicate"], true);
    let mut conflict = request.clone();
    conflict.args.insert("text".into(), json!("different"));
    assert!(!crate::dispatch::dispatch(&state.source, &conflict).ok);
    state.drop_delivery_response.store(true, Ordering::Release);
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    assert!(
        delivery::process(&state.source, &client, &locks, &peer, id)
            .await
            .is_err(),
        "simulated response loss"
    );
    let target_path = target_store.ledger_path(&target.group_id).expect("ledger");
    let events = cccc_core::ledger::read_all(&target_path).expect("ledger");
    let messages = events
        .iter()
        .filter(|event| event.kind == "chat.message")
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data["to"], json!(["lead", "worker"]));
    assert_eq!(
        messages[0].data["src_instance_id"],
        json!(message(&entry).source.instance_id)
    );
    let received_path = messages[0].data["attachments"][0]["path"]
        .as_str()
        .expect("blob path");
    assert_eq!(
        std::fs::read(
            cccc_core::blobs::resolve(&state.target, &target.group_id, received_path)
                .expect("blob")
        )
        .expect("bytes"),
        b"file travels with its original message"
    );
    let mut resumed = cccc_core::connect_delivery::load(&state.source, &peer, id)
        .expect("load")
        .expect("pending");
    assert!(resumed.progress.needs_receipt);
    resumed.progress.next_attempt_at = None;
    cccc_core::connect_delivery::update_progress(
        &state.source,
        &peer,
        id,
        resumed.progress.clone(),
    )
    .expect("retry due");
    // After process restart, a changed recipient must not prevent receipt recovery.
    target.actors.remove(1);
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("worker"))
        .expect("recreated worker");
    target_store.save(&target).expect("save");
    let reopened = HomeLayout::from_path(state.source.root()).expect("reopen");
    let segments = target_path
        .parent()
        .expect("group")
        .join("state/ledger/segments");
    std::fs::create_dir_all(&segments).expect("segments");
    std::fs::rename(&target_path, segments.join("ledger.0001.jsonl"))
        .expect("archive committed message");
    delivery::process(&reopened, &client, &locks, &peer, id)
        .await
        .expect("recover original receipt");
    assert_eq!(
        state.deliveries.load(Ordering::Acquire),
        1,
        "receipt recovery must not POST the body again"
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&reopened)
            .expect("queue")
            .is_empty()
    );
    let source_events =
        cccc_core::ledger::read_all(&source_store.ledger_path(&source.group_id).expect("ledger"))
            .expect("events");
    assert_eq!(
        source_events
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
    );
    let final_event = source_events
        .iter()
        .find(|event| event.kind == "chat.cross_group_receipt")
        .expect("final status");
    assert_eq!(final_event.data["status"], "sent");
    assert_eq!(final_event.data["remote_event_id"], messages[0].id);
    // Simulate a crash after terminal projection but before unlinking the outbox.
    cccc_core::connect_delivery::reserve(&reopened, &resumed).expect("resurrect active record");
    delivery::process(&reopened, &client, &locks, &peer, id)
        .await
        .expect("cleanup");
    assert_eq!(state.deliveries.load(Ordering::Acquire), 1);
    assert!(
        cccc_core::connect_delivery::pending_ids(&reopened)
            .expect("queue")
            .is_empty()
    );
    assert!(
        crate::dispatch::dispatch(&state.source, &request).ok,
        "stable retry after finalization"
    );
    server.abort();
}

#[tokio::test]
async fn connect_scheduler_gives_a_quiet_peer_a_turn_before_busy_peers_drain() {
    let mut peers = Vec::new();
    for _ in 0..5 {
        peers.push(setup().await);
    }
    peers.sort_by(|a, b| a.2.cmp(&b.2));
    let home = peers[0].1.source.clone();
    let mut snapshot = connect::load(&home).expect("snapshot").expect("binding");
    let directory = snapshot.directory.as_mut().expect("directory");
    let source_identity = directory.instances[0].clone();
    directory.instances.truncate(1);
    for (_, state, peer, _) in &peers {
        let mut target_snapshot = connect::load(&state.target)
            .expect("target")
            .expect("binding");
        let target_directory = target_snapshot.directory.as_mut().expect("directory");
        let device_id = format!("device-{peer}");
        membership::update(&state.target, |state| {
            state.device_id = Some(device_id.clone());
            Ok(())
        })
        .expect("distinct target device");
        target_snapshot.device_id = device_id.clone();
        target_directory.device_id = device_id.clone();
        target_directory.instances[1].device_id = device_id;
        directory
            .instances
            .push(target_directory.instances[1].clone());
        target_directory.instances[0] = source_identity.clone();
        connect::save(&state.target, &target_snapshot).expect("target directory");
    }
    connect::save(&home, &snapshot).expect("source directory");
    let store = GroupStore::new(home.clone()).expect("store");
    let source = store.create("Sender", "").expect("source");
    let client = client();
    let mut quiet_delivery = String::new();
    for (index, (_, state, peer, _)) in peers.iter().enumerate() {
        let target = GroupStore::new(state.target.clone())
            .expect("store")
            .create("Receiver", "")
            .expect("target");
        refresh_catalog(&home, &client, peer)
            .await
            .expect("catalog");
        // Four busy destinations can fill every worker slot. A fifth destination
        // must not wait for any of those queues to empty before its first turn.
        for n in 0..if index == 4 { 1 } else { 32 } {
            let accepted = crate::dispatch::dispatch(
                &home,
                &send_request(
                    &source.group_id,
                    &target.group_id,
                    peer,
                    json!(["user"]),
                    &format!("fair-{index}-{n}"),
                ),
            );
            assert!(accepted.ok, "{accepted:?}");
            if index == 4 {
                quiet_delivery = accepted.result["delivery_id"].as_str().expect("id").into();
            }
        }
    }
    let worker = tokio::spawn(delivery::run(
        home.clone(),
        client,
        crate::dispatch_concurrency::DispatchLocks::default(),
    ));
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while !cccc_core::connect_delivery::pending_ids(&home)
            .expect("queue")
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    worker.abort();
    let _ = worker.await;
    for (_, _, _, server) in &peers {
        server.abort();
    }
    assert!(drained.is_ok(), "all healthy peers must drain");
    let events = cccc_core::ledger::read_all(&store.ledger_path(&source.group_id).expect("path"))
        .expect("ledger");
    let receipts: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "chat.cross_group_receipt")
        .collect();
    assert_eq!(receipts.len(), 129);
    assert!(receipts.iter().all(|event| event.data["status"] == "sent"));
    let position = receipts
        .iter()
        .position(|event| event.data["delivery_id"] == quiet_delivery)
        .expect("quiet peer receipt");
    assert!(
        position < 16,
        "quiet peer waited behind {position} receipts from busy peers"
    );
}

#[tokio::test]
async fn connect_scheduler_drains_ready_work_without_waiting_a_tick_per_message() {
    let (_temp, state, peer, server) = setup().await;
    let source = GroupStore::new(state.source.clone())
        .expect("store")
        .create("Source", "")
        .expect("source");
    let target = GroupStore::new(state.target.clone())
        .expect("store")
        .create("Target", "")
        .expect("target");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let mut ready = Vec::new();
    let mut delayed = String::new();
    for n in 0..13 {
        let accepted = crate::dispatch::dispatch(
            &state.source,
            &send_request(
                &source.group_id,
                &target.group_id,
                &peer,
                json!(["user"]),
                &format!("burst-{n}"),
            ),
        );
        assert!(accepted.ok, "{accepted:?}");
        let id = accepted.result["delivery_id"]
            .as_str()
            .expect("id")
            .to_owned();
        if n == 12 {
            let mut entry = cccc_core::connect_delivery::load(&state.source, &peer, &id)
                .expect("load")
                .expect("entry");
            entry.progress.next_attempt_at =
                Some((Utc::now() + ChronoDuration::seconds(60)).to_rfc3339());
            cccc_core::connect_delivery::update_progress(&state.source, &peer, &id, entry.progress)
                .expect("backoff");
            delayed = id;
        } else {
            ready.push(id);
        }
    }
    let worker = tokio::spawn(delivery::run(
        state.source.clone(),
        client,
        crate::dispatch_concurrency::DispatchLocks::default(),
    ));
    // This is a scheduling bound, not a microbenchmark: a one-second delay per
    // ready message cannot finish, while loopback delivery has ample headroom.
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        while ready.iter().any(|id| {
            cccc_core::connect_delivery::load(&state.source, &peer, id)
                .expect("queue")
                .is_some()
        }) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    worker.abort();
    let _ = worker.await;
    server.abort();
    assert!(
        drained.is_ok(),
        "ready work must resume when a worker finishes"
    );
    assert_eq!(state.deliveries.load(Ordering::Acquire), 12);
    assert!(
        cccc_core::connect_delivery::load(&state.source, &peer, &delayed)
            .expect("queue")
            .is_some(),
        "completion must not bypass a persisted retry deadline"
    );
}

#[tokio::test]
async fn connect_scheduler_isolates_a_broken_record_and_retires_deleted_source_work() {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("store");
    let source = source_store.create("Source", "").expect("source");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let target = target_store.create("Target", "").expect("target");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let corrupt = state
        .source
        .root()
        .join("state/connect/outbox")
        .join(&peer)
        .join("00000000-0000-0000-0000-000000000000.json");
    std::fs::create_dir_all(corrupt.parent().expect("peer queue")).expect("queue");
    std::fs::write(&corrupt, b"{broken").expect("inject isolated record corruption");
    let accepted = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["user"]),
            "healthy",
        ),
    );
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let home = state.source.clone();
    let worker_client = client.clone();
    let worker = tokio::spawn(async move {
        delivery::run(
            home,
            worker_client,
            crate::dispatch_concurrency::DispatchLocks::default(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while cccc_core::connect_delivery::load(&state.source, &peer, &id)
            .expect("queue")
            .is_some()
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("healthy message is not starved by an earlier corrupt record");
    worker.abort();
    let _ = worker.await;
    assert!(corrupt.exists(), "do not conceal corrupted persisted work");
    std::fs::remove_file(corrupt).expect("remove own fault");
    let accepted = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["user"]),
            "deleted",
        ),
    );
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"].as_str().expect("id");
    let posts = state.deliveries.load(Ordering::Acquire);
    source_store
        .delete(&source.group_id)
        .expect("explicitly delete own fixture Group");
    delivery::process(
        &state.source,
        &client,
        &crate::dispatch_concurrency::DispatchLocks::default(),
        &peer,
        id,
    )
    .await
    .expect("retire deleted source work");
    assert_eq!(state.deliveries.load(Ordering::Acquire), posts);
    assert!(
        !source_store
            .group_dir(&source.group_id)
            .expect("path")
            .exists(),
        "no ledger resurrection"
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&state.source)
            .expect("queue")
            .is_empty()
    );
    server.abort();
}

#[tokio::test]
async fn connect_delivery_rejects_recreated_actor_without_retargeting_and_recovers_source_projection()
 {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("store");
    let source = source_store.create("Source", "").expect("source");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let mut target = target_store.create("Target", "").expect("target");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("lead")).expect("lead");
    target_store.save(&target).expect("save");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let path = source_store.ledger_path(&source.group_id).expect("ledger");
    if path.exists() {
        std::fs::remove_file(&path).expect("fixture ledger");
    }
    std::fs::create_dir(&path).expect("inject projection I/O failure");
    let accepted = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["@foreman"]),
            "projection",
        ),
    );
    // Lookup also happens before acceptance; use a clean source, then remove its
    // projection to exercise recovery from the persisted acceptance checkpoint.
    assert!(
        !accepted.ok,
        "an unreadable existing ledger must fail before acceptance"
    );
    std::fs::remove_dir(&path).expect("remove own fault");
    let accepted = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["@foreman"]),
            "projection",
        ),
    );
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"].as_str().expect("id");
    let mut pending = cccc_core::connect_delivery::load(&state.source, &peer, id)
        .expect("load")
        .expect("pending");
    // Only our empty fixture Group's projected message is removed; no live history.
    std::fs::remove_file(&path).expect("simulate missing projection");
    pending.progress.source_projected = false;
    cccc_core::connect_delivery::update_progress(&state.source, &peer, id, pending.progress)
        .expect("checkpoint");
    target.actors.clear();
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("lead")).expect("replacement");
    target_store.save(&target).expect("save");
    delivery::process(
        &state.source,
        &client,
        &crate::dispatch_concurrency::DispatchLocks::default(),
        &peer,
        id,
    )
    .await
    .expect("terminal rejection");
    let source_events = cccc_core::ledger::read_all(&path).expect("recovered source");
    assert_eq!(
        source_events
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
    );
    assert!(
        source_events
            .iter()
            .any(|event| event.kind == "chat.cross_group_receipt"
                && event.data["status"] == "failed")
    );
    assert!(
        !cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events")
            .iter()
            .any(|event| event.kind == "chat.message")
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&state.source)
            .expect("queue")
            .is_empty()
    );
    server.abort();
}

#[tokio::test]
async fn connect_reply_and_follow_up_use_original_identity_without_a_reverse_catalog() {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("source store");
    let mut source = source_store.create("Source", "").expect("source");
    cccc_core::actors::add(&mut source, cccc_contracts::Actor::new("lead")).expect("source lead");
    source_store.save(&source).expect("save");
    let target_store = GroupStore::new(state.target.clone()).expect("target store");
    let mut target = target_store.create("Target", "").expect("target");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("lead")).expect("target lead");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("worker")).expect("worker");
    target_store.save(&target).expect("save");
    let client = client();
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("source discovery only");
    let mut request = send_request(
        &source.group_id,
        &target.group_id,
        &peer,
        json!(["worker"]),
        "question",
    );
    request.args.insert("by".into(), json!("lead"));
    request
        .args
        .insert("message_mode".into(), json!("request_reply"));
    let accepted = crate::dispatch::dispatch(&state.source, &request);
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"].as_str().expect("id");
    let original = cccc_core::connect_delivery::load(&state.source, &peer, id)
        .expect("load")
        .expect("queued");
    let cccc_contracts::connect_message::ConnectWork::Message(original) = original.work else {
        panic!("message")
    };
    delivery::process(&state.source, &client, &locks, &peer, id)
        .await
        .expect("question delivered");
    let completed_retry = crate::dispatch::dispatch(&state.source, &request);
    assert!(completed_retry.ok, "{completed_retry:?}");
    assert_eq!(completed_retry.result["delivery_state"], "sent");
    assert_eq!(completed_retry.result["queued"], false);
    assert_eq!(
        completed_retry.result["source_event"]["data"]["client_id"],
        request.args["client_id"]
    );
    let target_events =
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events");
    let received = target_events
        .iter()
        .find(|event| {
            event.kind == "chat.message" && event.data["connect_message"]["delivery_id"] == id
        })
        .expect("received question");
    assert!(
        connect_catalog::load(&state.target, &original.source.instance_id)
            .expect("cache")
            .is_none()
    );
    let mut reply=DaemonRequest{v:1,op:"reply".into(),args:json!({"group_id":target.group_id,"by":"worker","reply_to":received.id,"text":"answer from original worker","client_id":"answer","message_mode":"send"}).as_object().expect("args").clone()};
    let before_preflight =
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events");
    let mut preflight = reply.clone();
    preflight.op = "message_upload_preflight".into();
    preflight.args.insert("operation".into(), json!("reply"));
    preflight.args.insert("text".into(), json!(""));
    preflight.args.insert("has_attachments".into(), json!(true));
    let checked = crate::dispatch::dispatch(&state.target, &preflight);
    assert!(checked.ok, "{checked:?}");
    assert_eq!(checked.result["ready"], true);
    assert_eq!(
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events"),
        before_preflight
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&state.target)
            .expect("queue")
            .is_empty()
    );
    preflight.args.insert("by".into(), json!("lead"));
    assert!(
        !crate::dispatch::dispatch(&state.target, &preflight).ok,
        "preflight uses the same original-recipient check"
    );
    let answer = crate::dispatch::dispatch(&state.target, &reply);
    assert!(answer.ok, "{answer:?}");
    assert_eq!(
        answer.result["source_event"]["data"]["dst_instance_name"],
        "0"
    );
    let answer_id = answer.result["delivery_id"].as_str().expect("answer id");
    assert_eq!(answer.result["event"]["data"]["reply_to"], received.id);
    let statuses = |home: &HomeLayout, group: &str, event: &str| {
        crate::dispatch::dispatch(
            home,
            &DaemonRequest {
                v: 1,
                op: "ledger_statuses".into(),
                args: json!({"group_id":group,"event_ids":[event]})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        )
    };
    assert_eq!(
        statuses(&state.target, &target.group_id, &received.id).result["statuses"][&received.id]["obligation_status"]
            ["worker"]["replied"],
        true,
        "local request is answered"
    );
    delivery::process(
        &state.target,
        &client,
        &locks,
        &original.source.instance_id,
        answer_id,
    )
    .await
    .expect("answer delivered without reverse discovery");
    let source_status = statuses(&state.source, &source.group_id, &original.source_event_id);
    assert_eq!(
        source_status.result["statuses"][&original.source_event_id]["connect_delivery"]["state"],
        "sent"
    );
    let window = crate::dispatch::dispatch(
        &state.source,
        &DaemonRequest {
            v: 1,
            op: "ledger_tail".into(),
            args: json!({"group_id":source.group_id,"limit":50,"with_obligation_status":true})
                .as_object()
                .expect("args")
                .clone(),
        },
    );
    assert!(window.ok, "{window:?}");
    let decorated = window.result["events"]
        .as_array()
        .expect("events")
        .iter()
        .find(|event| event["id"] == original.source_event_id)
        .expect("original");
    assert_eq!(decorated["_connect_delivery"]["state"], "sent");
    assert_eq!(
        source_status.result["statuses"][&original.source_event_id]["obligation_status"]["worker"]
            ["replied"],
        true,
        "remote reply has its original Actor identity"
    );
    let source_events =
        cccc_core::ledger::read_all(&source_store.ledger_path(&source.group_id).expect("path"))
            .expect("events");
    let received_answer = source_events
        .iter()
        .find(|event| event.data.get("text") == Some(&json!("answer from original worker")))
        .expect("received answer");
    assert_eq!(received_answer.data["reply_to"], original.source_event_id);
    assert_eq!(received_answer.data["to"], json!(["lead"]));
    assert_eq!(received_answer.data["source_platform"], "cccc_connect");
    assert_eq!(received_answer.data["src_group_title"], "Target");
    assert_eq!(received_answer.data["source_user_id"], "worker");
    assert_eq!(received_answer.data["source_user_name"], "worker");
    assert_eq!(received_answer.data["src_instance_name"], "1");
    reply.args.insert("by".into(), json!("lead"));
    reply.args.insert("client_id".into(), json!("wrong-actor"));
    assert!(
        !crate::dispatch::dispatch(&state.target, &reply).ok,
        "another local Actor cannot take over a bound reply"
    );
    let follow_up=crate::dispatch::dispatch(&state.source,&DaemonRequest{v:1,op:"reply".into(),args:json!({"group_id":source.group_id,"by":"lead","reply_to":original.source_event_id,"text":"follow-up from original sender","client_id":"follow-up","message_mode":"send"}).as_object().expect("args").clone()});
    assert!(follow_up.ok, "{follow_up:?}");
    delivery::process(
        &state.source,
        &client,
        &locks,
        &peer,
        follow_up.result["delivery_id"].as_str().expect("id"),
    )
    .await
    .expect("follow-up delivered");
    let events =
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events");
    let follow_up = events
        .iter()
        .find(|event| event.data.get("text") == Some(&json!("follow-up from original sender")))
        .expect("follow-up");
    assert_eq!(follow_up.data["reply_to"], received.id);
    assert_eq!(follow_up.data["to"], json!(["worker"]));
    server.abort();
}

#[derive(Clone)]
pub(crate) struct Fixture {
    pub(crate) source: HomeLayout,
    pub(crate) target: HomeLayout,
    wrong_identity: Arc<AtomicBool>,
    retire_after_response: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    drop_delivery_response: Arc<AtomicBool>,
    drop_cancel_response: Arc<AtomicBool>,
    deliveries: Arc<AtomicUsize>,
}

#[tokio::test]
async fn connect_file_send_keeps_scope_and_idempotency_and_transfers_more_than_a_small_proof() {
    let (temp, state, peer, server) = setup().await;
    let store = GroupStore::new(state.source.clone()).expect("store");
    let mut source = store.create("Files", "").expect("source");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project");
    source.active_scope_key = "fixture".into();
    source.scopes.push(cccc_core::group::Scope {
        scope_key: "fixture".into(),
        url: root.to_string_lossy().into_owned(),
        label: String::new(),
        git_remote: String::new(),
    });
    store.save(&source).expect("save scope");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let target = target_store.create("Receiver", "").expect("target");
    let bytes = "跨实例附件\n".repeat(3000).into_bytes();
    std::fs::write(root.join("payload.txt"), &bytes).expect("source file");
    std::fs::write(temp.path().join("outside.txt"), b"outside").expect("outside fixture");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let mut request = send_request(
        &source.group_id,
        &target.group_id,
        &peer,
        json!(["user"]),
        "file-retry",
    );
    request.op = "connect_send_files".into();
    request.args.insert("paths".into(), json!(["payload.txt"]));
    request.args.remove("text");
    let mut invalid = request.clone();
    invalid
        .args
        .insert("paths".into(), json!(["../outside.txt"]));
    assert!(
        !crate::dispatch::dispatch(&state.source, &invalid).ok,
        "source scope cannot be escaped"
    );
    let accepted = crate::dispatch::dispatch(&state.source, &request);
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"].as_str().expect("id");
    assert_eq!(
        accepted.result["source_event"]["data"]["text"],
        "[files] payload.txt"
    );
    std::fs::remove_file(root.join("payload.txt"))
        .expect("delete original after durable acceptance");
    let repeated = crate::dispatch::dispatch(&state.source, &request);
    assert!(
        repeated.ok,
        "stable retry does not reopen the original source file: {repeated:?}"
    );
    assert_eq!(repeated.result["delivery_id"], id);
    delivery::process(
        &state.source,
        &client,
        &crate::dispatch_concurrency::DispatchLocks::default(),
        &peer,
        id,
    )
    .await
    .expect("file transfer");
    let events =
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("events");
    let event = events
        .iter()
        .find(|event| event.kind == "chat.message")
        .expect("received file");
    let blob = event.data["attachments"][0]["path"].as_str().expect("blob");
    assert_eq!(
        std::fs::read(
            cccc_core::blobs::resolve(&state.target, &target.group_id, blob).expect("resolve")
        )
        .expect("read"),
        bytes
    );
    let large = root.join("too-large.bin");
    std::fs::File::create(&large)
        .expect("file")
        .set_len(cccc_core::connect_delivery::MAX_ATTACHMENT_BYTES + 1)
        .expect("length");
    request.args.insert("client_id".into(), json!("too-large"));
    request
        .args
        .insert("paths".into(), json!(["too-large.bin"]));
    assert!(
        !crate::dispatch::dispatch(&state.source, &request).ok,
        "bounded attachment admission"
    );
    assert!(
        cccc_core::connect_delivery::pending_ids(&state.source)
            .expect("queue")
            .is_empty()
    );
    server.abort();
}

async fn identity(
    State(state): State<Fixture>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let home = if state.wrong_identity.load(Ordering::Acquire) {
        &state.source
    } else {
        &state.target
    };
    Json(
        serde_json::to_value(connect_peer::identity_proof(home, &query["nonce"]).expect("proof"))
            .expect("json"),
    )
}

async fn receive(
    State(state): State<Fixture>,
    headers: axum::http::HeaderMap,
    Json(operation): Json<ConnectPeerOperation>,
) -> Json<Value> {
    let encoded = headers[cccc_contracts::connect::CONNECT_PROOF_HEADER]
        .to_str()
        .expect("header");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .expect("proof bytes");
    let envelope = ConnectPeerRequest {
        proof: serde_json::from_slice(&bytes).expect("proof"),
        operation,
    };
    let is_delivery = matches!(envelope.operation, ConnectPeerOperation::Deliver { .. });
    let is_cancel = matches!(envelope.operation, ConnectPeerOperation::Cancel { .. });
    if is_delivery {
        state.deliveries.fetch_add(1, Ordering::AcqRel);
    }
    state.requests.fetch_add(1, Ordering::AcqRel);
    let response = crate::dispatch::dispatch(
        &state.target,
        &DaemonRequest {
            v: 1,
            op: "connect_peer_receive".into(),
            args: json!({"group_id":envelope.operation.target_group_id(),"envelope":envelope})
                .as_object()
                .expect("args")
                .clone(),
        },
    );
    if state.retire_after_response.load(Ordering::Acquire) {
        membership::update(&state.source, |value| {
            value.disabled = true;
            Ok(())
        })
        .expect("retire");
    }
    if (is_delivery && state.drop_delivery_response.swap(false, Ordering::AcqRel))
        || (is_cancel && state.drop_cancel_response.swap(false, Ordering::AcqRel))
    {
        return Json(json!({"fixture":"response lost after ledger commit"}));
    }
    Json(serde_json::to_value(response).expect("response"))
}

pub(crate) async fn setup() -> (
    tempfile::TempDir,
    Fixture,
    String,
    tokio::task::JoinHandle<()>,
) {
    let temp = tempfile::tempdir().expect("fixture");
    let source = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let target = HomeLayout::from_path(temp.path().join("b")).expect("home");
    for home in [&source, &target] {
        home.initialize().expect("initialize");
    }
    let state = Fixture {
        source,
        target,
        wrong_identity: Arc::new(AtomicBool::new(false)),
        retire_after_response: Arc::new(AtomicBool::new(false)),
        requests: Arc::new(AtomicUsize::new(0)),
        drop_delivery_response: Arc::new(AtomicBool::new(false)),
        drop_cancel_response: Arc::new(AtomicBool::new(false)),
        deliveries: Arc::new(AtomicUsize::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let origin = format!("http://{}", listener.local_addr().expect("address"));
    let source_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("source listener");
    let source_origin = format!(
        "http://{}",
        source_listener.local_addr().expect("source address")
    );
    let now = Utc::now();
    let instances = [&state.source, &state.target]
        .iter()
        .enumerate()
        .map(|(index, home)| {
            let key = InstanceIdentity::load_or_create(home).expect("key");
            ConnectInstance {
                instance_id: key.peer_id,
                public_key: key.public_key_b64,
                device_id: format!("device-{index}"),
                client_version: env!("CARGO_PKG_VERSION").into(),
                public_origin: Some(if index == 0 {
                    source_origin.clone()
                } else {
                    origin.clone()
                }),
                display_name: index.to_string(),
                registered_at: now.to_rfc3339(),
            }
        })
        .collect::<Vec<_>>();
    for (home, own) in [&state.source, &state.target].into_iter().zip(&instances) {
        membership::save(
            home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some("http://localhost:7654".into()),
                device_id: Some(own.device_id.clone()),
                device_token: Some("fixture-only".into()),
                ..Default::default()
            },
        )
        .expect("membership");
        connect::save(
            home,
            &ConnectSnapshot {
                account_origin: "http://localhost:7654".into(),
                device_id: own.device_id.clone(),
                instance_id: own.instance_id.clone(),
                directory: Some(ConnectDirectory {
                    protocol_version: 1,
                    account_id: "same-account".into(),
                    device_id: own.device_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: (now + ChronoDuration::seconds(120)).to_rfc3339(),
                    instances: instances.clone(),
                }),
                ..Default::default()
            },
        )
        .expect("snapshot");
    }
    let app = |endpoint| {
        Router::new()
            .route("/api/v1/connect/identity", get(identity))
            .route(
                "/api/v1/connect/peer",
                post(receive).layer(axum::extract::DefaultBodyLimit::max(14 * 1024 * 1024)),
            )
            .with_state(endpoint)
    };
    let mut reverse = state.clone();
    std::mem::swap(&mut reverse.source, &mut reverse.target);
    let target_app = app(state.clone());
    let source_app = app(reverse);
    let server = tokio::spawn(async move {
        tokio::select! {
            result=axum::serve(listener,target_app)=>result.expect("target server"),
            result=axum::serve(source_listener,source_app)=>result.expect("source server"),
        }
    });
    (temp, state, instances[1].instance_id.clone(), server)
}

#[tokio::test]
async fn signed_peer_catalog_crosses_real_http_without_web_tokens_and_survives_restart() {
    let (_temp, state, peer, server) = setup().await;
    let group = GroupStore::new(state.target.clone())
        .expect("store")
        .create("Peer workspace", "")
        .expect("group");
    let client: PeerClient = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client")
        .into();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("refresh");
    let reopened = HomeLayout::from_path(state.source.root()).expect("reopen");
    let catalog = connect_catalog::load(&reopened, &peer)
        .expect("cache")
        .expect("catalog");
    assert_eq!(catalog.groups[0].group_id, group.group_id);
    assert!(connect_catalog::is_fresh(&catalog));
    let mut offline = catalog.clone();
    offline.checked_at = (Utc::now() - ChronoDuration::seconds(300)).to_rfc3339();
    connect_catalog::save(&reopened, &offline).expect("old navigation metadata");
    let old = connect_catalog::load(&reopened, &peer)
        .expect("read")
        .expect("old catalog");
    assert!(!connect_catalog::is_fresh(&old));
    assert_eq!(old.groups[0].group_id, group.group_id);
    assert_eq!(state.requests.load(Ordering::Acquire), 1);
    assert!(!state.source.root().join("access_tokens.yaml").exists());
    let request=DaemonRequest {v:1,op:"connect_catalog".into(),args:json!({"group_id":GroupStore::new(reopened.clone()).expect("store").create("Source", "").expect("source").group_id,"instance_id":peer,"by":"user"}).as_object().expect("args").clone()};
    for _ in 0..3 {
        assert!(crate::dispatch::dispatch(&reopened, &request).ok);
    }
    assert_eq!(
        state.requests.load(Ordering::Acquire),
        1,
        "reading the cache does not contact a peer"
    );
    server.abort();
}

#[tokio::test]
async fn endpoint_identity_precedes_disclosure_and_late_revoked_results_are_not_saved() {
    let (_temp, state, peer, server) = setup().await;
    let client: PeerClient = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client")
        .into();
    state.wrong_identity.store(true, Ordering::Release);
    assert!(
        refresh_catalog(&state.source, &client, &peer)
            .await
            .is_err()
    );
    assert_eq!(
        state.requests.load(Ordering::Acquire),
        0,
        "wrong endpoint never receives a catalog request"
    );
    state.wrong_identity.store(false, Ordering::Release);
    state.retire_after_response.store(true, Ordering::Release);
    assert!(
        refresh_catalog(&state.source, &client, &peer)
            .await
            .is_err()
    );
    assert!(!state.source.root().join("state/connect/catalog").exists());
    server.abort();
}

#[tokio::test]
async fn connect_failure_notification_recovers_after_final_receipt_and_respects_sender_generation()
{
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("store");
    let mut source = source_store.create("Source", "").expect("source");
    let mut sender = cccc_contracts::Actor::new("worker");
    sender.enabled = false;
    cccc_core::actors::add(&mut source, sender).expect("sender");
    source_store.save(&source).expect("source");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let target = target_store.create("Target", "").expect("target");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let path = source_store.ledger_path(&source.group_id).expect("ledger");
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    for (terminal, recreate) in [("failed", false), ("unconfirmed", true)] {
        let mut request = send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["user"]),
            terminal,
        );
        request.args.insert("by".into(), json!("worker"));
        let accepted = crate::dispatch::dispatch(&state.source, &request);
        assert!(accepted.ok, "{accepted:?}");
        let id = accepted.result["delivery_id"].as_str().expect("id");
        let entry = cccc_core::connect_delivery::load(&state.source, &peer, id)
            .expect("load")
            .expect("entry");
        // Crash checkpoint: terminal evidence exists, notification and cleanup do not.
        let mut final_event =
            cccc_contracts::Event::new("chat.cross_group_receipt", &source.group_id);
        final_event.by = "system".into();
        final_event.data = json!({"client_id":format!("connect:final:{id}"),"source_event_id":entry.source_event.id,"transport":"connect","status":terminal,"error":"isolated failure"}).as_object().expect("receipt").clone();
        cccc_core::ledger::append(&path, &final_event).expect("final checkpoint");
        if recreate {
            cccc_core::actors::remove(&mut source, "worker").expect("remove original");
            let mut replacement = cccc_contracts::Actor::new("worker");
            replacement.enabled = false;
            cccc_core::actors::add(&mut source, replacement).expect("replacement");
            source_store.save(&source).expect("save replacement");
        }
        delivery::process(&state.source, &client, &locks, &peer, id)
            .await
            .expect("finish notification and cleanup");
        assert!(
            cccc_core::connect_delivery::load(&state.source, &peer, id)
                .expect("load")
                .is_none()
        );
        // Another restart with a surviving active file cannot duplicate the notification.
        cccc_core::connect_delivery::reserve(&state.source, &entry)
            .expect("surviving queue fixture");
        delivery::process(&state.source, &client, &locks, &peer, id)
            .await
            .expect("idempotent recovery");
        let events = cccc_core::ledger::read_all(&path).expect("events");
        let notifications = events
            .iter()
            .filter(|event| {
                event.kind == "system.notify" && event.data["context"]["delivery_id"] == id
            })
            .collect::<Vec<_>>();
        assert_eq!(notifications.len(), 1);
        let notice = notifications[0];
        assert_eq!(
            notice.data["target_actor_id"],
            if recreate { "user" } else { "worker" }
        );
        assert_eq!(notice.data["im_visibility"], "internal");
        assert_eq!(notice.data["related_event_id"], entry.source_event.id);
        assert_eq!(
            cccc_core::inbox::is_for_actor(&source, notice, "worker"),
            !recreate
        );
        assert_eq!(notice.data["context"]["state"], terminal);
    }
    assert_eq!(
        state.deliveries.load(Ordering::Acquire),
        0,
        "terminal recovery never resends the message"
    );
    server.abort();
}

fn message(
    entry: &cccc_contracts::connect_message::ConnectOutboxEntry,
) -> &cccc_contracts::connect_message::ConnectMessage {
    let cccc_contracts::connect_message::ConnectWork::Message(message) = &entry.work else {
        panic!("message fixture")
    };
    message
}

#[tokio::test]
async fn connect_cancellation_waits_for_original_without_starving_it_and_converges_both_ends() {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("store");
    let source = source_store.create("Source", "").expect("source");
    let target_store = GroupStore::new(state.target.clone()).expect("store");
    let mut target = target_store.create("Target", "").expect("target");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("worker")).expect("worker");
    target_store.save(&target).expect("target");
    let client = client();
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let cancel_request = |group: &str, event: &str, by: &str| DaemonRequest {
        v: 1,
        op: "reply_request_cancel".into(),
        args: json!({"group_id":group,"source_event_id":event,"by":by})
            .as_object()
            .expect("args")
            .clone(),
    };
    let statuses = |home: &HomeLayout, group: &str, event: &str| {
        crate::dispatch::dispatch(
            home,
            &DaemonRequest {
                v: 1,
                op: "ledger_statuses".into(),
                args: json!({"group_id":group,"event_ids":[event]})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        )
    };
    for reverse in [false, true] {
        let mut request = send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["worker"]),
            if reverse {
                "reverse-cancel"
            } else {
                "forward-cancel"
            },
        );
        request
            .args
            .insert("message_mode".into(), json!("request_reply"));
        let accepted = crate::dispatch::dispatch(&state.source, &request);
        assert!(accepted.ok, "{accepted:?}");
        let message_id = accepted.result["delivery_id"].as_str().expect("message ID");
        let source_event_id = accepted.result["source_event"]["id"]
            .as_str()
            .expect("source ID");
        let mut cancelled = None;
        if !reverse {
            let result = crate::dispatch::dispatch(
                &state.source,
                &cancel_request(&source.group_id, source_event_id, "user"),
            );
            assert!(result.ok, "{result:?}");
            let id = result.result["delivery_id"].as_str().expect("cancel ID");
            assert_eq!(
                delivery::process(&state.source, &client, &locks, &peer, id)
                    .await
                    .expect("wait for original"),
                Duration::from_secs(5)
            );
            assert!(
                !cccc_core::ledger::read_all(
                    &target_store.ledger_path(&target.group_id).expect("path")
                )
                .expect("events")
                .iter()
                .any(|event| event.kind == "chat.reply_request.cancelled")
            );
            cancelled = Some(result);
        }
        delivery::process(&state.source, &client, &locks, &peer, message_id)
            .await
            .expect("original is not blocked by waiting cancellation");
        let received =
            cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
                .expect("events")
                .into_iter()
                .find(|event| {
                    event
                        .data
                        .get("connect_message")
                        .is_some_and(|message| message["delivery_id"] == message_id)
                })
                .expect("received");
        let (home, remote, cancellation) = if reverse {
            assert!(
                !crate::dispatch::dispatch(
                    &state.target,
                    &cancel_request(&target.group_id, &received.id, "worker")
                )
                .ok,
                "recipient Actor cannot cancel the sender's request"
            );
            let result = crate::dispatch::dispatch(
                &state.target,
                &cancel_request(&target.group_id, &received.id, "user"),
            );
            assert!(result.ok, "{result:?}");
            let local_id =
                accepted.result["source_event"]["data"]["connect_message"]["source"]["instance_id"]
                    .as_str()
                    .expect("local ID")
                    .to_owned();
            (&state.target, local_id, result)
        } else {
            (
                &state.source,
                peer.clone(),
                cancelled.expect("accepted cancellation"),
            )
        };
        let id = cancellation.result["delivery_id"]
            .as_str()
            .expect("cancel ID");
        let mut entry = cccc_core::connect_delivery::load(home, &remote, id)
            .expect("load")
            .expect("entry");
        entry.progress.next_attempt_at = None;
        cccc_core::connect_delivery::update_progress(home, &remote, id, entry.progress.clone())
            .expect("make test work due");
        let own_group = if reverse {
            &target.group_id
        } else {
            &source.group_id
        };
        let own_event = if reverse {
            &received.id
        } else {
            source_event_id
        };
        assert_eq!(
            statuses(home, own_group, own_event).result["statuses"][own_event]["connect_cancellation"]
                ["state"],
            "queued"
        );
        if !reverse {
            state.drop_cancel_response.store(true, Ordering::Release);
            assert!(
                delivery::process(home, &client, &locks, &remote, id)
                    .await
                    .is_err(),
                "lose response after target commit"
            );
            entry.progress = cccc_core::connect_delivery::load(home, &remote, id)
                .expect("load")
                .expect("pending")
                .progress;
            assert!(entry.progress.needs_receipt);
            entry.progress.next_attempt_at = None;
            cccc_core::connect_delivery::update_progress(home, &remote, id, entry.progress.clone())
                .expect("retry after restart");
            let path = target_store
                .ledger_path(&target.group_id)
                .expect("target ledger");
            let segments = path.parent().expect("group").join("state/ledger/segments");
            std::fs::create_dir_all(&segments).expect("segments");
            std::fs::rename(&path, segments.join("ledger.0001.jsonl"))
                .expect("archive accepted control");
        }
        let reopened = HomeLayout::from_path(home.root()).expect("reopen sender");
        delivery::process(&reopened, &client, &locks, &remote, id)
            .await
            .expect("cancellation delivered");
        assert_eq!(
            statuses(home, own_group, own_event).result["statuses"][own_event]["connect_cancellation"]
                ["state"],
            "sent"
        );
        assert_eq!(
            statuses(&state.source, &source.group_id, source_event_id).result["statuses"]
                [source_event_id]["obligation_status"]["worker"]["cancelled"],
            true
        );
        assert_eq!(
            statuses(&state.target, &target.group_id, &received.id).result["statuses"]
                [&received.id]["obligation_status"]["worker"]["cancelled"],
            true
        );
        // A surviving active file after final commit only finishes cleanup.
        cccc_core::connect_delivery::reserve(home, &entry).expect("surviving active fixture");
        delivery::process(home, &client, &locks, &remote, id)
            .await
            .expect("repeat recovery");
        let receiver = if reverse {
            &state.source
        } else {
            &state.target
        };
        let group = if reverse {
            &source.group_id
        } else {
            &target.group_id
        };
        let events = cccc_core::ledger::read_all(
            &GroupStore::new(receiver.clone())
                .expect("store")
                .ledger_path(group)
                .expect("path"),
        )
        .expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event
                    .data
                    .get("connect_cancel")
                    .is_some_and(|cancel| cancel["delivery_id"] == id))
                .count(),
            1
        );
    }
    server.abort();
}

#[tokio::test]
async fn connect_cancellation_deadline_reports_truthful_failure_and_retires_active_work() {
    use cccc_contracts::connect_message::{ConnectCancellation, ConnectOutboxEntry, ConnectWork};
    let (_temp, state, peer, server) = setup().await;
    let store = GroupStore::new(state.source.clone()).expect("store");
    let source = store.create("Source", "").expect("source");
    let target = GroupStore::new(state.target.clone())
        .expect("store")
        .create("Target", "")
        .expect("target");
    let client = client();
    refresh_catalog(&state.source, &client, &peer)
        .await
        .expect("catalog");
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    for (uncertain, expected) in [(false, "failed"), (true, "unconfirmed")] {
        let mut request = send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["user"]),
            expected,
        );
        request
            .args
            .insert("message_mode".into(), json!("request_reply"));
        let accepted = crate::dispatch::dispatch(&state.source, &request);
        assert!(accepted.ok, "{accepted:?}");
        let original_entry = cccc_core::connect_delivery::load(
            &state.source,
            &peer,
            accepted.result["delivery_id"].as_str().expect("id"),
        )
        .expect("load")
        .expect("entry");
        let original = message(&original_entry);
        // Restore an expired accepted control checkpoint, without waiting fifteen minutes.
        let mut event =
            cccc_contracts::Event::new("chat.reply_request.cancelled", &source.group_id);
        event.by = "user".into();
        let now = Utc::now();
        let cancel = ConnectCancellation {
            connection_id: None,
            delivery_id: uuid::Uuid::new_v4().to_string(),
            account_origin: original.account_origin.clone(),
            account_id: original.account_id.clone(),
            source: original.source.clone(),
            target: original.target.clone(),
            sender: original.sender.clone(),
            source_event_id: event.id.clone(),
            original_delivery_id: original.delivery_id.clone(),
            original_message_sha256: cccc_core::connect_delivery::digest(original),
            original_source_event_id: original.source_event_id.clone(),
            original_deliver_before: original.deliver_before.clone(),
            created_at: (now - ChronoDuration::seconds(901)).to_rfc3339(),
            deliver_before: (now - ChronoDuration::seconds(1)).to_rfc3339(),
        };
        event.data = json!({"source_event_id":original.source_event_id,"connect_cancel":cancel})
            .as_object()
            .expect("data")
            .clone();
        let id = cancel.delivery_id.clone();
        let entry = ConnectOutboxEntry {
            work: ConnectWork::Cancel(Box::new(cancel)),
            source_event: event,
            progress: cccc_contracts::connect_message::ConnectDeliveryProgress {
                needs_receipt: uncertain,
                ..Default::default()
            },
        };
        cccc_core::connect_delivery::reserve(&state.source, &entry).expect("restore expired work");
        delivery::process(&state.source, &client, &locks, &peer, &id)
            .await
            .expect("bounded completion");
        assert!(
            cccc_core::connect_delivery::load(&state.source, &peer, &id)
                .expect("load")
                .is_none()
        );
        let events =
            cccc_core::ledger::read_all(&store.ledger_path(&source.group_id).expect("path"))
                .expect("events");
        let final_event = events
            .iter()
            .find(|event| {
                event.kind == "chat.cross_group_receipt" && event.data["delivery_id"] == id
            })
            .expect("final");
        assert_eq!(final_event.data["status"], expected);
        assert_eq!(final_event.data["action"], "cancel");
        assert_eq!(
            final_event.data["original_event_id"],
            original.source_event_id
        );
        let notice = events
            .iter()
            .find(|event| {
                event.kind == "system.notify" && event.data["context"]["delivery_id"] == id
            })
            .expect("notice");
        assert_eq!(notice.data["related_event_id"], original.source_event_id);
        assert_eq!(notice.data["target_actor_id"], "user");
        let status = crate::dispatch::dispatch(
            &state.source,
            &DaemonRequest {
                v: 1,
                op: "ledger_statuses".into(),
                args: json!({"group_id":source.group_id,"event_ids":[original.source_event_id]})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        );
        assert_eq!(
            status.result["statuses"][&original.source_event_id]["connect_cancellation"]["state"],
            expected
        );
    }
    assert_eq!(
        state.deliveries.load(Ordering::Acquire),
        0,
        "expired work never sends a chat"
    );
    server.abort();
}

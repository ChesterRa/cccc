use super::tests::{client, send_request, setup};
use super::*;
use cccc_contracts::{DaemonRequest, connect_groups::*};
use cccc_core::{GroupStore, connect_groups};
use serde_json::json;

#[tokio::test]
async fn direct_pair_pins_routing_even_when_a_broader_account_grant_exists() {
    use cccc_contracts::direct::DirectListener;
    use cccc_core::direct;
    let (_temp, state, peer, server) = setup().await;
    let source = GroupStore::new(state.source.clone())
        .expect("store")
        .create("A", "")
        .expect("group");
    let target = GroupStore::new(state.target.clone())
        .expect("store")
        .create("B", "")
        .expect("group");
    direct::configure(
        &state.source,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("configure");
    let text = direct::invite(&state.source, &source.group_id).expect("invite");
    let id = direct::join(&state.target, &target.group_id, &text).expect("join");
    let relation = direct::load(&state.target)
        .expect("store")
        .relations
        .remove(0);
    direct::hello(
        &state.source,
        &id,
        &relation.invitation.expect("invite").secret,
        &relation.local,
        &relation.local.public_key,
    )
    .expect("claim");
    assert!(connect_peer::binding(&state.source, &peer).is_ok());
    assert!(
        connect_peer::group_binding(&state.source, &peer, &source.group_id, &target.group_id)
            .is_err()
    );
    direct::set_state(&state.source, &source.group_id, &id, true).expect("approve");
    assert_eq!(
        connect_peer::group_binding(&state.source, &peer, &source.group_id, &target.group_id)
            .expect("direct")
            .group
            .expect("scope")
            .id,
        id
    );
    direct::set_state(&state.source, &source.group_id, &id, false).expect("revoke");
    assert!(
        connect_peer::group_binding(&state.source, &peer, &source.group_id, &target.group_id)
            .is_err()
    );
    direct::remove(&state.source, &source.group_id, &id).expect("remove");
    assert!(
        connect_peer::group_binding(&state.source, &peer, &source.group_id, &target.group_id)
            .expect("future account route")
            .group
            .is_none()
    );
    assert!(
        connect_peer::scoped_binding(&state.source, &peer, Some(&id)).is_err(),
        "old work cannot acquire the new route"
    );
    server.abort();
}

#[tokio::test]
async fn external_group_messages_replies_files_and_revocation_share_the_durable_pipeline() {
    let (_temp, state, peer, server) = setup().await;
    let source_store = GroupStore::new(state.source.clone()).expect("source store");
    let source = source_store.create("Selected source", "").expect("source");
    let other_source = source_store
        .create("Unshared source", "")
        .expect("other source");
    let target_store = GroupStore::new(state.target.clone()).expect("target store");
    let mut target = target_store.create("Selected target", "").expect("target");
    cccc_core::actors::add(&mut target, cccc_contracts::Actor::new("worker")).expect("recipient");
    target_store.save(&target).expect("target");
    target_store
        .create("Unshared target", "")
        .expect("other target");
    let mut endpoints = Vec::new();
    for (index, (home, group)) in [(&state.source, &source), (&state.target, &target)]
        .into_iter()
        .enumerate()
    {
        let mut snapshot = connect::load(home).expect("snapshot").expect("linked");
        let directory = snapshot.directory.as_mut().expect("directory");
        directory.account_id = format!("member-{index}");
        directory
            .instances
            .retain(|i| i.instance_id == snapshot.instance_id);
        endpoints.push(ConnectGroupEndpoint {
            account_id: directory.account_id.clone(),
            instance: directory.instances[0].clone(),
            group_id: group.group_id.clone(),
            group_generation: connect_groups::generation(group),
            title: group.title.clone(),
        });
        connect::save(home, &snapshot).expect("separate account");
    }
    let link = ConnectGroupLink {
        id: uuid::Uuid::new_v4().to_string(),
        source: endpoints[0].clone(),
        target: endpoints[1].clone(),
    };
    for (index, home) in [&state.source, &state.target].into_iter().enumerate() {
        connect_groups::save(
            home,
            &ConnectGroupLinks {
                protocol_version: 1,
                account_origin: "http://localhost:7654".into(),
                account_id: format!("member-{index}"),
                device_id: endpoints[index].instance.device_id.clone(),
                issued_at: cccc_contracts::utc_now(),
                expires_at: (chrono::Utc::now() + chrono::Duration::seconds(119)).to_rfc3339(),
                links: vec![link.clone()],
            },
        )
        .expect("approved link");
    }
    let client = client();
    assert!(
        refresh_catalog(&state.source, &client, &peer)
            .await
            .is_err(),
        "no instance-wide access"
    );
    refresh_catalog_scoped(&state.source, &client, &peer, Some(&link.id))
        .await
        .expect("scoped catalogue");
    let cache = connect_catalog::load_scoped(&state.source, &peer, Some(&link.id))
        .expect("cache")
        .expect("catalogue");
    assert_eq!(cache.groups.len(), 1);
    assert_eq!(cache.groups[0].group_id, target.group_id);
    let discover = |group: &str| {
        crate::dispatch::dispatch(
            &state.source,
            &DaemonRequest {
                v: 1,
                op: "connect_catalog".into(),
                args: json!({"group_id":group,"by":"user"})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        )
    };
    assert_eq!(
        discover(&source.group_id).result["external_groups"]
            .as_array()
            .expect("external")
            .len(),
        1
    );
    assert_eq!(
        discover(&other_source.group_id).result["external_groups"],
        json!([])
    );
    assert!(
        !crate::dispatch::dispatch(
            &state.source,
            &send_request(
                &other_source.group_id,
                &target.group_id,
                &peer,
                json!(["worker"]),
                "not-transitive"
            )
        )
        .ok
    );
    let mut request = send_request(
        &source.group_id,
        &target.group_id,
        &peer,
        json!(["worker"]),
        "external-question",
    );
    request
        .args
        .insert("message_mode".into(), json!("request_reply"));
    let bytes = b"scoped attachment";
    let blob = cccc_core::blobs::store(&state.source, &source.group_id, bytes).expect("blob");
    request.args.insert(
        "attachments".into(),
        json!([{"path":blob.path,"name":"note.txt","mime_type":"text/plain"}]),
    );
    let accepted = crate::dispatch::dispatch(&state.source, &request);
    assert!(accepted.ok, "{accepted:?}");
    let id = accepted.result["delivery_id"].as_str().expect("id");
    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    delivery::process(&state.source, &client, &locks, &peer, id)
        .await
        .expect("sent");
    let events =
        cccc_core::ledger::read_all(&target_store.ledger_path(&target.group_id).expect("path"))
            .expect("ledger");
    let received = events
        .iter()
        .find(|e| e.kind == "chat.message")
        .expect("received");
    assert_eq!(received.data["connect_message"]["connection_id"], link.id);
    let attachment = received.data["attachments"][0]["path"]
        .as_str()
        .expect("attachment");
    assert_eq!(
        std::fs::read(
            cccc_core::blobs::resolve(&state.target, &target.group_id, attachment).expect("path")
        )
        .expect("file"),
        bytes
    );
    let reply=DaemonRequest{v:1,op:"reply".into(),args:json!({"group_id":target.group_id,"by":"worker","reply_to":received.id,"text":"external answer","client_id":"external-answer"}).as_object().expect("args").clone()};
    let answer = crate::dispatch::dispatch(&state.target, &reply);
    assert!(answer.ok, "{answer:?}");
    delivery::process(
        &state.target,
        &client,
        &locks,
        &link.source.instance.instance_id,
        answer.result["delivery_id"].as_str().expect("answer id"),
    )
    .await
    .expect("reply delivered");
    let events =
        cccc_core::ledger::read_all(&source_store.ledger_path(&source.group_id).expect("path"))
            .expect("ledger");
    assert!(
        events
            .iter()
            .any(|e| e.data.get("text") == Some(&json!("external answer")))
    );

    // A known revocation retires unsent work without contacting the peer or waiting for its delivery deadline.
    let queued = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["worker"]),
            "pending-revocation",
        ),
    );
    assert!(queued.ok, "{queued:?}");
    let uncertain = crate::dispatch::dispatch(
        &state.source,
        &send_request(
            &source.group_id,
            &target.group_id,
            &peer,
            json!(["worker"]),
            "uncertain-revocation",
        ),
    );
    assert!(uncertain.ok, "{uncertain:?}");
    let uncertain_id = uncertain.result["delivery_id"].as_str().expect("id");
    let mut entry = cccc_core::connect_delivery::load(&state.source, &peer, uncertain_id)
        .expect("read")
        .expect("queued");
    entry.progress.needs_receipt = true;
    cccc_core::connect_delivery::update_progress(
        &state.source,
        &peer,
        uncertain_id,
        entry.progress,
    )
    .expect("POST may have escaped");
    let mut grant = connect_groups::load(&state.source)
        .expect("read")
        .expect("grant");
    grant.links.clear();
    connect_groups::save(&state.source, &grant).expect("revoke");
    assert!(
        !state
            .source
            .root()
            .join(format!("state/connect/catalog/group-{}.json", link.id))
            .exists(),
        "revocation removes the retired connection catalogue without touching history"
    );
    delivery::process(
        &state.source,
        &client,
        &locks,
        &peer,
        queued.result["delivery_id"].as_str().expect("id"),
    )
    .await
    .expect("retired");
    delivery::process(&state.source, &client, &locks, &peer, uncertain_id)
        .await
        .expect("uncertain work retired");
    assert!(
        cccc_core::connect_delivery::pending_ids(&state.source)
            .expect("queue")
            .is_empty()
    );
    assert!(
        connect_catalog::load_scoped(&state.source, &peer, Some(&link.id))
            .expect("load")
            .is_none()
    );
    let events =
        cccc_core::ledger::read_all(&source_store.ledger_path(&source.group_id).expect("path"))
            .expect("ledger");
    assert!(
        events.iter().any(|e| e.kind == "chat.cross_group_receipt"
            && e.data.get("status") == Some(&json!("failed")))
    );
    assert!(events.iter().any(|e| e.kind == "chat.cross_group_receipt"
        && e.data.get("status") == Some(&json!("unconfirmed"))));
    server.abort();
}

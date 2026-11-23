use super::*;
use crate::{client_receive, hub, network};
use cccc_contracts::{Actor, ActorRuntime, local_session::LocalSessionRuntime};
use cccc_core::{
    GroupStore, HomeLayout, actors, ledger,
    local_sessions::{self, NativeHost, NativeSession},
    session_nodes::ClientDescriptor,
};
use iroh::SecretKey;

async fn take_offer(hub: &Hub, receiver: &Value) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let result = daemon(hub, "local_session_receiver_next", receiver.clone())
                .await
                .expect("active receiver");
            if !result["handoff"].is_null() {
                return result["handoff"].clone();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("delivery worker offers the message")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handoff_offered_before_source_reload_does_not_cross_into_new_revision() {
    retry_after_native_reload(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_rejects_a_stale_offer_before_sync_and_the_hub_retries_it() {
    retry_after_native_reload(true).await;
}

async fn retry_after_native_reload(device_reloads_before_sync: bool) {
    let temp = tempfile::tempdir().expect("isolated Home");
    let home = HomeLayout::from_path(temp.path().join("hub")).expect("Home");
    home.initialize().expect("initialize");
    let ticket = hub::create_invitation(&home, 0).await.expect("gateway");
    let address = SessionNodeEndpoint {
        endpoint_id: ticket.endpoint_id,
        addresses: ticket.addresses,
        relay_url: None,
    };
    let (builder, destination) = network::connect_builder(SecretKey::generate(), &address, None)
        .expect("direct endpoint configuration");
    let endpoint = builder.bind().await.expect("node endpoint");
    let node_id = endpoint.id().to_string();
    let node = tokio::time::timeout(
        Duration::from_secs(10),
        endpoint.connect(destination, SESSION_NODE_ALPN),
    )
    .await
    .expect("direct connection deadline")
    .expect("direct connection");
    let hello = protocol::call(
        &node,
        &Request::Hello {
            version: SESSION_NODE_PROTOCOL,
            name: "Revision fixture".into(),
            invite_id: Some(ticket.invite_id),
            secret: Some(ticket.secret),
        },
    )
    .await
    .expect("authenticate");

    let hub = Arc::clone(hub::hubs().lock().await.get(home.root()).expect("hub"));
    let host = hub.connections.lock().await[&node_id].1.clone();
    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while daemon(&hub, "ping", json!({})).await.is_err() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("daemon startup");
    let device = HomeLayout::from_path(temp.path().join("node")).expect("node Home");
    let owner = NativeHost::capture(std::process::id()).expect("fixture owner");
    let runtime = LocalSessionRuntime::Pi;
    let mut native = local_sessions::write(
        &device,
        NativeSession {
            source_id: runtime.source_id(&owner.key()),
            runtime,
            native_session_id: "fixture-thread".into(),
            host: owner.clone(),
            cwd: "/node/project".into(),
            title: "Session fixture".into(),
            origin: None,
            revision: "offered-revision".into(),
            context_epoch: 0,
            codex_home: None,
            codex_can_receive: false,
            codex_program: None,
            claude_config_dir: None,
            pi_agent_dir: None,
            opencode_config_dir: None,
            opencode_server: None,
            retired: None,
        },
    )
    .expect("original native source");
    let client = ClientDescriptor {
        endpoint: address,
        node_id: node_id.clone(),
        grant_id: hello["peer"]["grant_id"].as_str().expect("grant").into(),
        secret: "fixture-broker".into(),
        owner,
        address: "127.0.0.1:1".parse().expect("fixture broker address"),
        connected: true,
        memberships: Vec::new(),
    };
    session_nodes::set_client(&device, &client).expect("connector state");
    let registered = SessionNodeSource {
        source: native.source(true),
        session_revision: native.revision.clone(),
        alive: true,
    };
    let source = session_nodes::update_sources(&home, &hub.gateway, &node_id, vec![registered])
        .expect("initial registration")
        .remove(0);
    let groups = GroupStore::new(home.clone()).expect("Groups");
    let mut group = groups.create("Revision test", "").expect("Group");
    group.running = true;
    let mut actor = Actor::new("member");
    actor.runtime = ActorRuntime::Pi;
    actor
        .external_session_id
        .clone_from(&source.source.source_id);
    actors::add(&mut group, actor).expect("Actor");
    groups.save(&group).expect("membership");
    let receiver = json!({"source_id":source.source.source_id,"connection_id":"fixture-receiver"});
    daemon(&hub, "local_session_receiver_attach", receiver.clone())
        .await
        .expect("attach hub receiver");
    let sent = daemon(
        &hub,
        "message_send",
        json!({"by":"user","group_id":group.group_id,"to":["member"],"text":"An offered handoff","message_mode":"send"}),
    )
    .await
    .expect("message enqueued by the real daemon");
    let event_ids = json!([sent["event"]["id"]]);
    let handoff = take_offer(&hub, &receiver).await;
    assert_eq!(handoff["source_revision"], "offered-revision");
    assert_eq!(handoff["event_ids"], event_ids);

    let mut ack = receiver.clone();
    ack["handoff_id"] = handoff["handoff_id"].clone();
    native.revision = "replacement-revision".into();
    native.native_session_id = "replacement-thread".into();
    let native = local_sessions::write(&device, native).expect("reload device source");
    let updated = SessionNodeSource {
        source: native.source(true),
        session_revision: native.revision.clone(),
        alive: true,
    };
    if device_reloads_before_sync {
        let receive = async {
            let (mut send, mut recv) = node.accept_bi().await.expect("old device stream");
            let request = protocol::read(&mut recv).await.expect("old device frame");
            let Request::Deliver { delivery } = &request else {
                panic!("expected a device delivery");
            };
            assert_eq!(delivery.source_revision, "offered-revision");
            let response = client_receive::incoming_delivery(
                &device,
                &cccc_client::DaemonClient::new(device.clone()),
                &client.secret,
                &DEVICE_SESSION_RUNTIMES,
                Ok(request),
            )
            .await
            .expect("device reports a definite result before native submission");
            protocol::write(&mut send, &response)
                .await
                .expect("device answer");
        };
        let (outcome, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(transfer(&hub, &node_id, &host, &source, &handoff), receive)
        })
        .await
        .expect("stale offer exchanged");
        assert!(matches!(
            outcome.expect("device rejection"),
            SessionNodeDeliveryOutcome::Unavailable
        ));
        assert!(
            !device
                .root()
                .join("state/session_nodes/deliveries.json")
                .exists()
        );
        assert!(
            session_nodes::client(&device)
                .expect("connector state")
                .expect("connector")
                .memberships
                .is_empty()
        );
    }
    session_nodes::update_sources(&home, &hub.gateway, &node_id, vec![updated])
        .expect("hub learns the current revision");
    if !device_reloads_before_sync {
        let forwarded = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                result = transfer(&hub, &node_id, &host, &source, &handoff) => {
                    assert!(matches!(result.expect("local rejection"), SessionNodeDeliveryOutcome::Unavailable));
                    false
                },
                stream = node.accept_bi() => { stream.expect("device stream"); true }
            }
        })
        .await;
        assert!(!forwarded.expect("delivery decision"));
        ack["state"] = json!("accepted");
        assert!(
            daemon(&hub, "local_session_receiver_ack", ack.clone())
                .await
                .is_err()
        );
    }
    ack["state"] = json!("unavailable");
    daemon(&hub, "local_session_receiver_ack", ack.clone())
        .await
        .expect("confirm that the old offer never reached the device");

    let fresh = take_offer(&hub, &receiver).await;
    assert_eq!(fresh["event_ids"], event_ids);
    assert_ne!(fresh["handoff_id"], handoff["handoff_id"]);
    assert_eq!(fresh["source_revision"], "replacement-revision");
    let transfer = transfer(&hub, &node_id, &host, &source, &fresh);
    let receive = async {
        let (mut send, mut recv) = node.accept_bi().await.expect("device stream for new offer");
        let request = protocol::read(&mut recv).await.expect("device frame");
        let Request::Deliver { delivery } = &request else {
            panic!("unexpected device frame");
        };
        assert_eq!(delivery.source_revision, "replacement-revision");
        assert_eq!(json!(delivery.event_ids), event_ids);
        assert_eq!(json!(delivery.handoff_id), fresh["handoff_id"]);
        let rejected = client_receive::incoming_delivery(
            &device,
            &cccc_client::DaemonClient::new(device.clone()),
            &client.secret,
            &LEGACY_DEVICE_SESSION_RUNTIMES,
            Ok(request),
        )
        .await
        .expect_err("a valid Pi source is outside this connection's runtime set");
        assert_eq!(
            rejected.to_string(),
            "Runtime unavailable on this connection"
        );
        protocol::write(&mut send, &json!({"outcome":{"status":"accepted"}}))
            .await
            .expect("device confirms the one offer");
    };
    let (outcome, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(transfer, receive)
    })
    .await
    .expect("new offer exchanged");
    assert!(matches!(
        outcome.expect("delivery outcome"),
        SessionNodeDeliveryOutcome::Accepted
    ));
    ack["state"] = json!("accepted");
    ack["handoff_id"] = fresh["handoff_id"].clone();
    daemon(&hub, "local_session_receiver_ack", ack)
        .await
        .expect("complete new offer");

    let ledger = groups.ledger_path(&group.group_id).expect("ledger");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let receipts = ledger::read_all(&ledger)
                .expect("ledger")
                .into_iter()
                .filter(|event| {
                    event.kind == "runtime.delivery"
                        && event.data["source_event_id"] == event_ids[0]
                })
                .collect::<Vec<_>>();
            let accepted = receipts
                .iter()
                .filter(|event| event.data["state"] == "accepted")
                .count();
            if accepted > 0 {
                assert!(receipts.iter().any(
                    |event| event.data["state"] == "failed" && event.data["retryable"] == true
                ));
                assert_eq!(accepted, 1);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("one durable acceptance for the original message");
    let empty = daemon(&hub, "local_session_receiver_next", receiver)
        .await
        .expect("receiver next after acceptance");
    assert!(empty["handoff"].is_null());
    hub::stop(&home).await;
    endpoint.close().await;
    daemon(&hub, "shutdown", json!({})).await.expect("shutdown");
    tokio::time::timeout(Duration::from_secs(5), daemon_task)
        .await
        .expect("daemon stops")
        .expect("daemon task")
        .expect("daemon result");
}

use super::{
    api::Api,
    created,
    homes::{self, Homes},
};
use axum::http::StatusCode;
use cccc_contracts::session_node_join::CreateRequest;
use cccc_core::{local_sessions::NativeHost, session_nodes};
use homes::output;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn join_rejects_unsafe_origins_legacy_hubs_and_redirected_claims() {
    let f = Homes::new();
    for bad in [
        "http://example.invalid",
        "https://private-input@hub.invalid",
        "https://hub.invalid/?secret=private-input",
    ] {
        let (ok, text) = output(f.join(bad, &[]), &["private-input"]).await;
        assert!(!ok && text.contains("HTTPS hub URL"));
    }
    assert!(
        !f.node.root().exists(),
        "invalid origins do not initialize the device identity"
    );
    let mut api = Api::start().await;
    let mut foreign = Api::start().await;
    let old = f.join(&api.origin, &[]);
    api.next("requests")
        .await
        .answer(StatusCode::NOT_FOUND, json!({}));
    let (ok, text) = output(old, &[]).await;
    assert!(!ok && text.contains("Update the hub") && text.contains("--ticket-stdin"));

    let malformed = f.join(&api.origin, &[]);
    let create = api.next("requests").await;
    let private: CreateRequest =
        serde_json::from_value(create.body.clone()).expect("private test claim");
    let id = uuid::Uuid::new_v4().to_string();
    let mut invalid = created(
        &id,
        &(chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
    );
    invalid["result"]["user_code"] = json!(private.device_code);
    create.answer(StatusCode::OK, invalid);
    let private_values = [&private.device_code[..]];
    let (ok, text) = tokio::select! {
        value=output(malformed,&private_values)=>value,
        _=api.calls.recv()=>panic!("a malformed public verification code must stop before polling"),
    };
    assert!(!ok && text.contains("invalid confirmation details"));

    let redirected = f.join(&api.origin, &[]);
    let create = api.next("requests").await;
    let value: CreateRequest = serde_json::from_value(create.body.clone()).expect("request body");
    let id = uuid::Uuid::new_v4().to_string();
    create.answer(
        StatusCode::OK,
        created(
            &id,
            &(chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
        ),
    );
    api.next("poll")
        .await
        .redirect(&format!("{}/capture", foreign.origin));
    let (ok, text) = output(redirected, &[&value.device_code]).await;
    assert!(!ok && text.contains("redirected"));
    assert!(
        tokio::time::timeout(Duration::from_millis(75), foreign.calls.recv())
            .await
            .is_err(),
        "no polling credential may cross origins"
    );
}

/// A legacy offline descriptor can coexist with a pending approval, but a different
/// connector that commits during the wait wins even if an approved ticket arrives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_join_preserves_a_newer_owner_and_a_live_owner_receives_no_request() {
    use cccc_contracts::session_node::{
        SESSION_NODE_PROTOCOL, SessionNodeEndpoint, SessionNodeTicket,
    };
    let mut f = Homes::new();
    f.start().await;
    let mut api = Api::start().await;
    let key = cccc_web::session_node::JoinTarget::prepare(&f.node, &api.origin)
        .expect("local key")
        .node_id();
    let owner = NativeHost::capture(std::process::id()).expect("fixture owner");
    let mut retired = owner.clone();
    retired.started.push_str("-retired");
    let mut descriptor = session_nodes::ClientDescriptor {
        endpoint: SessionNodeEndpoint {
            endpoint_id: key.clone(),
            addresses: vec!["127.0.0.1:1".into()],
            relay_url: None,
        },
        join_binding: None,
        node_id: key.clone(),
        grant_id: "old-grant".into(),
        owner: retired,
        address: "127.0.0.1:1".parse().expect("loopback broker"),
        secret: "old-broker".into(),
        connected: false,
        memberships: vec![],
    };
    session_nodes::set_client(&f.node, &descriptor).expect("persist offline legacy descriptor");
    assert!(
        !cccc_web::session_node::JoinTarget::prepare(&f.node, &api.origin)
            .expect("legacy descriptor")
            .can_resume(),
        "old descriptors without a binding need approval"
    );
    descriptor.join_binding = Some(session_nodes::JoinBinding {
        origin: format!("{}/", api.origin),
        hub_id: "different-authenticated-hub".into(),
    });
    session_nodes::set_client(&f.node, &descriptor).expect("controlled mismatched binding");
    let pending = f.join(&api.origin, &["--runtime", "codex"]);
    let request = api.next("requests").await;
    let claim: CreateRequest =
        serde_json::from_value(request.body.clone()).expect("new-origin claim");
    assert_eq!(claim.node_id, key);
    let id = uuid::Uuid::new_v4().to_string();
    let expiry = (chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339();
    request.answer(StatusCode::OK, created(&id, &expiry));
    let poll = api.next("poll").await;
    descriptor.owner = owner;
    descriptor.connected = true;
    descriptor.grant_id = "concurrently-committed-grant".into();
    descriptor.join_binding = Some(session_nodes::JoinBinding {
        origin: "https://another.example/".into(),
        hub_id: key.clone(),
    });
    session_nodes::set_client(&f.node, &descriptor).expect("newer live connector");
    let sig = f.descriptor_bytes();
    let reserved = tokio::net::UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("test-owned address");
    let ticket = SessionNodeTicket {
        v: SESSION_NODE_PROTOCOL,
        endpoint_id: key,
        addresses: vec![reserved.local_addr().expect("test address").to_string()],
        relay_url: None,
        relay_token: None,
        invite_id: id,
        secret: "unused-approval-secret".into(),
        expires_at: expiry,
    };
    let wire = cccc_web::session_node::encode_ticket(&ticket).expect("synthetic ticket");
    poll.answer(
        StatusCode::OK,
        json!({"ok":true,"result":{"status":"approved","ticket":wire}}),
    );
    let (ok, text) = output(pending, &[&claim.device_code, &wire, &ticket.secret]).await;
    assert!(
        !ok && text.contains("changed during pairing") && !text.contains("integration enabled")
    );
    let (ok, text) = output(f.join(&api.origin, &["--reauthorize"]), &[]).await;
    assert!(!ok && text.contains("already runs"));
    assert!(
        api.calls.try_recv().is_err(),
        "a running connector must not trigger an approval"
    );
    assert!(
        f.descriptor_bytes() == sig,
        "concurrent ownership must preserve the entire newer grant and origin"
    );
    f.close().await;
}

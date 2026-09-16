use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::Engine;
use cccc_contracts::connect::{ConnectDirectory, ConnectFrameProof, ConnectInstance};
use cccc_core::{
    HomeLayout,
    access_tokens::AccessTokenStore,
    connect::{ConnectSnapshot, save},
    instance_identity::InstanceIdentity,
    membership,
};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use tower::ServiceExt;

async fn stream_until(body: &mut axum::body::BodyDataStream, marker: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(18), async {
        loop {
            let chunk = body.next().await.expect("open stream").expect("SSE chunk");
            if String::from_utf8_lossy(&chunk).contains(marker) {
                break;
            }
        }
    })
    .await
    .expect("stream progress within access interval");
}

async fn embedded_stream_revocation(reason: &str) {
    let (_temp, a, b, _, _) = setup("https://b.test");
    let store = cccc_core::GroupStore::new(b.clone()).expect("store");
    let group = store.create("stream revocation", "").expect("group");
    let ledger = store.ledger_path(&group.group_id).expect("ledger");
    let token = AccessTokenStore::new(b.clone())
        .expect("tokens")
        .create("target admin", Vec::new(), true, None)
        .expect("admin");
    let app = cccc_web::app(b.clone());
    let mut proof = proof(&a, &b);
    if reason == "expiry" {
        proof.expires_at = (Utc::now() + chrono::Duration::seconds(3)).to_rfc3339();
        proof.signature = InstanceIdentity::load(&a)
            .expect("identity")
            .sign(&proof.signing_material())
            .expect("sign");
    }
    assert_eq!(
        app.clone()
            .oneshot(frame_request(&proof))
            .await
            .expect("frame")
            .status(),
        StatusCode::OK
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let mut request = format!(
        "ws://{}/api/v1/events/ws?connect_frame={}",
        listener.local_addr().expect("address"),
        proof.frame_id,
    )
    .into_client_request()
    .expect("socket request");
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", token.token).parse().expect("header"),
    );
    let socket_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, socket_app).await.expect("serve");
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("socket");
    for (id, channel) in [(1, "global"), (2, "ledger"), (3, "headless")] {
        socket
            .send(Message::Text(
                json!({
                    "type": "subscribe", "id": id, "channel": channel,
                    "group_id": group.group_id, "replay": true,
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("subscribe");
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut ready = std::collections::HashSet::new();
        while ready.len() < 3 {
            if let Message::Text(text) = socket.next().await.expect("open socket").expect("frame") {
                let value: Value = serde_json::from_str(&text).expect("packet");
                if value["type"] == "ready" {
                    ready.insert(value["id"].as_u64().expect("subscription id"));
                }
            }
        }
    })
    .await
    .expect("all embedded subscriptions ready");
    let mut streams = Vec::new();
    for path in [
        format!("/api/v1/events/stream?connect_frame={}", proof.frame_id),
        format!(
            "/api/v1/groups/{}/ledger/stream?connect_frame={}",
            group.group_id, proof.frame_id
        ),
        "/api/v1/events/stream".into(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("SSE");
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        stream_until(&mut body, "connected").await;
        streams.push(body);
    }
    let before = cccc_contracts::Event::new("chat.message", &group.group_id);
    cccc_core::ledger::append(&ledger, &before).expect("before revoke");
    for body in &mut streams {
        stream_until(body, &before.id).await;
    }
    match reason {
        "target" => {
            membership::update(&b, |state| {
                state.disabled = true;
                Ok(())
            })
            .expect("retire target");
        }
        "source" => {
            let mut snapshot = cccc_core::connect::load(&b)
                .expect("snapshot")
                .expect("linked");
            snapshot
                .directory
                .as_mut()
                .expect("directory")
                .instances
                .retain(|entry| entry.device_id != "device-a");
            save(&b, &snapshot).expect("retire source in current directory");
        }
        "expiry" => {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        _ => unreachable!(),
    }
    for body in &mut streams[..2] {
        stream_until(body, "auth_required").await;
        assert!(
            body.next().await.is_none(),
            "server ends revoked embedded stream"
        );
    }
    tokio::time::timeout(std::time::Duration::from_secs(18), async {
        let mut rejected = false;
        while let Some(message) = socket.next().await {
            match message.expect("frame") {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).expect("packet");
                    rejected |= value["type"] == "fatal" && value["code"] == "auth_required";
                }
                Message::Close(_) => {
                    assert!(
                        rejected,
                        "frame revocation rejects the entire multiplexed socket"
                    );
                    return;
                }
                _ => {}
            }
        }
        panic!("expected server close frame");
    })
    .await
    .expect("revoked embedded socket closes");
    let after = cccc_contracts::Event::new("chat.message", &group.group_id);
    cccc_core::ledger::append(&ledger, &after).expect("after revoke");
    stream_until(&mut streams[2], &after.id).await;
    server.abort();
}

#[tokio::test]
async fn embedded_streams_close_on_source_retirement_without_revoking_target_token() {
    embedded_stream_revocation("source").await;
}

#[tokio::test]
async fn embedded_streams_close_on_target_retirement_without_revoking_target_token() {
    embedded_stream_revocation("target").await;
}

#[tokio::test]
async fn embedded_streams_close_on_frame_expiry_without_revoking_target_token() {
    embedded_stream_revocation("expiry").await;
}

#[tokio::test]
async fn global_stream_rejects_missing_frame_authority_before_emitting_metadata() {
    let (_temp, _a, b, _, _) = setup("https://b.test");
    let token = AccessTokenStore::new(b.clone())
        .expect("tokens")
        .create("target admin", Vec::new(), true, None)
        .expect("admin");
    let response = cccc_web::app(b)
        .oneshot(
            Request::get("/api/v1/events/stream?connect_frame=unknown")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("SSE");
    let text = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec(),
    )
    .expect("text");
    assert!(text.contains("auth_required"));
    assert!(!text.contains("group_id"));
}

fn setup(origin_b: &str) -> (tempfile::TempDir, HomeLayout, HomeLayout, String, String) {
    let temp = tempfile::tempdir().expect("fixture directory");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("A home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("B home");
    let mut entries = Vec::new();
    for (home, device_id, origin) in [
        (&a, "device-a", "https://a.test"),
        (&b, "device-b", origin_b),
    ] {
        home.initialize().expect("initialize");
        let key = InstanceIdentity::load_or_create(home).expect("identity");
        membership::save(
            home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some("https://account.test".into()),
                device_id: Some(device_id.into()),
                device_token: Some("fixture-device-secret".into()),
                ..Default::default()
            },
        )
        .expect("binding");
        entries.push(ConnectInstance {
            instance_id: key.peer_id,
            device_id: device_id.into(),
            public_key: key.public_key_b64,
            client_version: env!("CARGO_PKG_VERSION").into(),
            public_origin: Some(origin.into()),
            display_name: device_id.into(),
            registered_at: Utc::now().to_rfc3339(),
        });
    }
    for (index, home) in [&a, &b].into_iter().enumerate() {
        let now = Utc::now();
        save(
            home,
            &ConnectSnapshot {
                account_origin: "https://account.test".into(),
                device_id: entries[index].device_id.clone(),
                instance_id: entries[index].instance_id.clone(),
                directory: Some(ConnectDirectory {
                    protocol_version: 1,
                    account_id: "owner".into(),
                    device_id: entries[index].device_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: (now + chrono::Duration::seconds(120)).to_rfc3339(),
                    instances: entries.clone(),
                }),
                ..Default::default()
            },
        )
        .expect("directory");
    }
    let token = AccessTokenStore::new(a.clone())
        .expect("tokens")
        .create("admin-a", Vec::new(), true, None)
        .expect("A token")
        .token;
    AccessTokenStore::new(b.clone())
        .expect("tokens")
        .create("admin-b", Vec::new(), true, None)
        .expect("B token");
    (temp, a, b, token, entries[1].instance_id.clone())
}

fn proof(a: &HomeLayout, b: &HomeLayout) -> ConnectFrameProof {
    let a_snapshot = cccc_core::connect::load(a)
        .expect("A snapshot")
        .expect("A binding");
    let b_snapshot = cccc_core::connect::load(b)
        .expect("B snapshot")
        .expect("B binding");
    let now = Utc::now();
    let mut proof = ConnectFrameProof {
        account_origin: a_snapshot.account_origin,
        source_instance_id: a_snapshot.instance_id,
        source_device_id: a_snapshot.device_id,
        target_instance_id: b_snapshot.instance_id,
        target_device_id: b_snapshot.device_id,
        parent_origin: "http://a.test".into(),
        frame_id: uuid::Uuid::new_v4().to_string(),
        nonce: uuid::Uuid::new_v4().to_string(),
        issued_at: now.to_rfc3339(),
        expires_at: (now + chrono::Duration::seconds(120)).to_rfc3339(),
        signature: String::new(),
    };
    proof.signature = InstanceIdentity::load_or_create(a)
        .expect("A key")
        .sign(&proof.signing_material())
        .expect("sign proof");
    proof
}

fn frame_request(proof: &ConnectFrameProof) -> Request<Body> {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(proof).expect("proof JSON"));
    Request::get(format!("/ui/connect/?proof={encoded}"))
        .body(Body::empty())
        .expect("frame request")
}

#[tokio::test]
async fn peer_proof_is_checked_before_polling_an_untrusted_request_body() {
    let (_temp, _a, b, _token, _id) = setup("https://b.test");
    let body = Body::from_stream(futures_util::stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
            panic!("unauthenticated peer body must never be polled");
        },
    ));
    let response = cccc_web::app(b)
        .oneshot(
            Request::post("/api/v1/connect/peer")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn frame_proof_changes_only_its_own_ancestors_and_never_authenticates_the_target() {
    let (_temp, a, b, token_a, _) = setup("https://b.test");
    let app = cccc_web::app(b.clone());
    let proof = proof(&a, &b);
    let response = app
        .clone()
        .oneshot(frame_request(&proof))
        .await
        .expect("frame response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self' http://a.test"
    );
    assert!(!response.headers().contains_key("x-frame-options"));
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let replay = app
        .clone()
        .oneshot(frame_request(&proof))
        .await
        .expect("replay");
    assert_eq!(replay.status(), StatusCode::FORBIDDEN);
    let ordinary = app
        .clone()
        .oneshot(Request::get("/ui/").body(Body::empty()).expect("request"))
        .await
        .expect("ordinary page");
    assert_eq!(
        ordinary.headers()["content-security-policy"],
        "frame-ancestors 'self'"
    );
    assert_eq!(ordinary.headers()["x-frame-options"], "SAMEORIGIN");
    let denied = app
        .oneshot(
            Request::get("/api/v1/connect")
                .header(header::AUTHORIZATION, format!("Bearer {token_a}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("B request");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tampering_wrong_bindings_and_retired_sources_cannot_open_frames() {
    let (_temp, a, b, _, _) = setup("https://b.test");
    let app = cccc_web::app(b.clone());
    let original = proof(&a, &b);
    let mut tampered = original.clone();
    tampered.parent_origin = "https://evil.test".into();
    assert_eq!(
        app.clone()
            .oneshot(frame_request(&tampered))
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut wrong = original.clone();
    wrong.target_device_id = "rebound-device".into();
    assert_eq!(
        app.clone()
            .oneshot(frame_request(&wrong))
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut snapshot = cccc_core::connect::load(&b)
        .expect("snapshot")
        .expect("binding");
    snapshot
        .directory
        .as_mut()
        .expect("directory")
        .instances
        .retain(|entry| entry.device_id != "device-a");
    save(&b, &snapshot).expect("retire A in account confirmation");
    assert_eq!(
        app.oneshot(frame_request(&original))
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn frame_renewal_preserves_scope_and_nested_resources_require_current_authority() {
    let (_temp, a, b, _, _) = setup("https://b.test");
    let app = cccc_web::app(b.clone());
    let mut proof = proof(&a, &b);
    proof.issued_at = (Utc::now() - chrono::Duration::seconds(2)).to_rfc3339();
    proof.expires_at = (Utc::now() + chrono::Duration::seconds(110)).to_rfc3339();
    let identity = InstanceIdentity::load_or_create(&a).expect("key");
    proof.signature = identity.sign(&proof.signing_material()).expect("sign");
    assert_eq!(
        app.clone()
            .oneshot(frame_request(&proof))
            .await
            .expect("open")
            .status(),
        StatusCode::OK
    );
    let renew = |proof: &ConnectFrameProof| {
        Request::post("/api/v1/connect/frame")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(proof).expect("proof")))
            .expect("request")
    };
    assert_eq!(
        app.clone()
            .oneshot(renew(&proof))
            .await
            .expect("replay")
            .status(),
        StatusCode::FORBIDDEN
    );
    proof.issued_at = Utc::now().to_rfc3339();
    proof.expires_at = (Utc::now() + chrono::Duration::seconds(110)).to_rfc3339();
    proof.nonce = uuid::Uuid::new_v4().to_string();
    proof.signature = identity
        .sign(&proof.signing_material())
        .expect("sign renewal");
    assert_eq!(
        app.clone()
            .oneshot(renew(&proof))
            .await
            .expect("renew")
            .status(),
        StatusCode::OK
    );
    let asset = || {
        Request::get(format!(
            "/api/v1/groups/g/presentation/slots/slot-1/asset?connect_frame={}",
            proof.frame_id
        ))
        .body(Body::empty())
        .expect("asset")
    };
    let response = app.clone().oneshot(asset()).await.expect("asset response");
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "frame proof never replaces target login"
    );
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self' http://a.test"
    );
    membership::update(&b, |state| {
        state.disabled = true;
        Ok(())
    })
    .expect("retire target");
    let response = app.clone().oneshot(asset()).await.expect("retired asset");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self'"
    );
    assert_eq!(
        app.oneshot(renew(&proof))
            .await
            .expect("retired renewal")
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_exhibit_cannot_be_opened_as_an_administrator_workbench() {
    let (_temp, a, b, _, _) = setup("https://b.test");
    let request = frame_request(&proof(&a, &b));
    let response = cccc_web::app_with_mode(b, cccc_web::WebMode::Exhibit)
        .oneshot(request)
        .await
        .expect("exhibit");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self'"
    );
}

#[tokio::test]
async fn different_ports_on_one_hostname_do_not_form_a_browser_credential_boundary() {
    let (_temp, a, b, _, _) = setup("https://b.test:9443");
    let mut proof = proof(&a, &b);
    proof.parent_origin = "https://b.test:8443".into();
    proof.signature = InstanceIdentity::load_or_create(&a)
        .expect("key")
        .sign(&proof.signing_material())
        .expect("signature");
    let response = cccc_web::app(b)
        .oneshot(frame_request(&proof))
        .await
        .expect("frame");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self'"
    );
}

#[tokio::test]
async fn opening_confirms_the_actual_target_key_and_restricted_entries_cannot_start_it() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let origin_b = format!("http://{}", listener.local_addr().expect("address"));
    let (_temp, a, b, token_a, target_id) = setup(&origin_b);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, cccc_web::app(b))
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .expect("server");
    });
    let app = cccc_web::app(a.clone());
    let request = |token: &str| {
        Request::post("/api/v1/connect/open")
            .header(header::HOST, "a.test")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"instance_id":target_id,"frame_id":uuid::Uuid::new_v4().to_string()})
                    .to_string(),
            ))
            .expect("open request")
    };
    let scoped = AccessTokenStore::new(a)
        .expect("tokens")
        .create("limited", vec!["g_one".into()], false, None)
        .expect("scoped token");
    assert_eq!(
        app.clone()
            .oneshot(request(&scoped.token))
            .await
            .expect("denied")
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = app.oneshot(request(&token_a)).await.expect("open");
    assert_eq!(response.status(), StatusCode::OK);
    let data: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("JSON");
    assert_eq!(data["result"]["origin"], origin_b);
    assert!(
        data["result"]["url"]
            .as_str()
            .expect("url")
            .starts_with(&format!("{origin_b}/ui/connect/?proof="))
    );
    assert!(!data.to_string().contains(&token_a));
    stop_tx.send(()).expect("stop");
    server.await.expect("server task");
}

#[tokio::test]
async fn external_group_management_requires_admin_but_resource_checks_require_only_signed_selection()
 {
    let (_temp, home, _, _, _) = setup("https://b.test");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("Selected Group", "")
        .expect("Group");
    let limited = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("limited", vec![group.group_id.clone()], false, None)
        .expect("token");
    let app = cccc_web::app(home.clone());
    for method in ["GET", "POST"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!(
                        "/api/v1/connect/groups?group_id={}",
                        group.group_id
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {}", limited.token))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"group_id":group.group_id}).to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let mut check = cccc_contracts::connect_groups::ConnectGroupCheck {
        ticket: cccc_core::connect_groups::ticket(&home, &group.group_id).expect("selection"),
        nonce: uuid::Uuid::new_v4().to_string(),
    };
    let request = |check: &cccc_contracts::connect_groups::ConnectGroupCheck| {
        Request::post("/api/v1/connect/group-check")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(check).expect("body")))
            .expect("request")
    };
    let response = app.clone().oneshot(request(&check)).await.expect("proof");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("JSON");
    assert_eq!(body["title"], "Selected Group");
    assert_eq!(body["nonce"], check.nonce);
    let response = cccc_web::app_with_mode(home.clone(), cccc_web::WebMode::Exhibit)
        .oneshot(request(&check))
        .await
        .expect("exhibit");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let path = home
        .root()
        .join("groups")
        .join(&group.group_id)
        .join("group.yaml");
    let original = std::fs::read(&path).expect("fixture YAML");
    std::fs::write(&path, "v: [invalid YAML").expect("transient read failure");
    let response = app
        .clone()
        .oneshot(request(&check))
        .await
        .expect("read failure");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("error");
    assert_eq!(error["error"]["code"], "connect_group_unavailable");
    std::fs::write(&path, original).expect("restore");
    assert_eq!(
        app.clone()
            .oneshot(request(&check))
            .await
            .expect("recovery")
            .status(),
        StatusCode::OK
    );
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    store.delete(&group.group_id).expect("delete");
    store.import(group).expect("replacement with same ID");
    let response = app
        .clone()
        .oneshot(request(&check))
        .await
        .expect("stale selection");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("error");
    assert_eq!(error["error"]["code"], "connect_group_denied");
    check.ticket.title = "forged selection".into();
    assert_eq!(
        app.clone()
            .oneshot(request(&check))
            .await
            .expect("forgery")
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = app
        .oneshot(
            Request::post("/api/v1/connect/group-check")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(" ".repeat(4097)))
                .expect("oversize"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

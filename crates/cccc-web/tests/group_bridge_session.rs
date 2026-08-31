mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use cccc_client::DaemonClient;
use cccc_contracts::{Actor, ActorRole, DaemonRequest, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout, Scope, ledger};
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

#[path = "group_bridge_session/web_delivery.rs"]
mod web_delivery;
use web_delivery::complete_web_delivery_over_session;

#[path = "group_bridge_session/bearer_downgrade.rs"]
mod bearer_downgrade;
#[path = "group_bridge_session/session_auth_support.rs"]
mod session_auth_support;
#[path = "group_bridge_session/v2_auth.rs"]
mod v2_auth;

async fn complete_client_initiated_delivery(socket: &mut TestSocket) {
    socket
        .send(WsMessage::Text(
            json!({
                "type":"request",
                "request_id":"client-request",
                "op":"remote_send",
                "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "src_group_id":"g_sender",
                "idempotency_key":"client-delivery",
                "payload":{
                    "source_by":"sender-agent",
                    "src_event_id":"client-source-message",
                    "text":"hello from the outbound Rust session",
                    "to":["foreman"],
                    "message_mode":"request_reply"
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("client request");
    let response = next_socket_json(socket).await;
    assert_eq!(response["type"], "response");
    assert_eq!(response["response_to"], "client-request");
    assert_eq!(
        response["result"]["receipt"]["status"], "sent",
        "{response}"
    );
    let remote_source_event_id = response["result"]["event"]["id"]
        .as_str()
        .expect("remote source event id");
    socket
        .send(WsMessage::Text(
            json!({
                "type":"request",
                "request_id":"client-cancellation",
                "op":"reply_request_cancel",
                "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "src_group_id":"g_sender",
                "idempotency_key":"client-cancellation",
                "payload":{
                    "source_group_id":"g_sender",
                    "source_message_event_id":"client-source-message",
                    "source_cancel_event_id":"client-cancel-event",
                    "remote_source_event_id":remote_source_event_id
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("client cancellation");
    let cancellation = next_socket_json(socket).await;
    assert_eq!(cancellation["type"], "response", "{cancellation}");
    assert_eq!(
        cancellation["response_to"], "client-cancellation",
        "{cancellation}"
    );
    assert_eq!(
        cancellation["result"]["event"]["kind"],
        "chat.reply_request.cancelled"
    );
}

async fn session_ready(home: &HomeLayout, group_id: &str, peer_id: &str) -> bool {
    let response = DaemonClient::new(home.clone())
        .call(&daemon_request(
            "group_bridge_session_ready",
            json!({"group_id":group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id}),
        ))
        .await
        .expect("ready call");
    response.ok && response.result["ready"] == true
}

async fn wait_for_session_ready(home: &HomeLayout, group_id: &str, peer_id: &str, expected: bool) {
    for _ in 0..100 {
        if session_ready(home, group_id, peer_id).await == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("session readiness did not become {expected}");
}

async fn complete_daemon_delivery(
    home: &HomeLayout,
    socket: &mut TestSocket,
    group_id: &str,
    peer_id: &str,
    key: &str,
) {
    let client = DaemonClient::new(home.clone());
    let request = daemon_request(
        "group_bridge_session_deliver",
        json!({"group_id":group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"operation":"remote_send","idempotency_key":key,"payload":{"text":key},"timeout_ms":2_000}),
    );
    let task = tokio::spawn(async move { client.call(&request).await });
    let frame = next_socket_json(socket).await;
    assert_eq!(frame["type"], "request");
    assert_eq!(frame["op"], "remote_send");
    socket.send(WsMessage::Text(json!({"type":"response","response_to":frame["request_id"],"result":{"ok":true,"receipt":{"status":"sent","remote_event_id":format!("remote-{key}")}}}).to_string().into())).await.expect("response");
    let response = task.await.expect("join").expect("delivery call");
    assert!(response.ok, "{response:?}");
    assert_eq!(
        response.result["receipt"]["remote_event_id"],
        format!("remote-{key}")
    );
}

fn daemon_request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_default(),
    }
}

fn seed_foreman(home: &HomeLayout, group_id: &str) {
    GroupStore::new(home.clone())
        .and_then(|store| {
            store.mutate(group_id, |group| {
                let mut actor = Actor::new("foreman");
                actor.role = Some(ActorRole::Foreman);
                group.actors.push(actor);
                Ok(())
            })
        })
        .expect("seed foreman");
}

fn test_peer_id(public: &[u8; 32]) -> String {
    let mut protobuf = vec![0x08, 0x01, 0x12, 32];
    protobuf.extend_from_slice(public);
    let mut bytes = vec![0x00, protobuf.len() as u8];
    bytes.extend(protobuf);
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let zeroes = bytes.iter().take_while(|byte| **byte == 0).count();
    let mut digits = vec![0u8];
    for byte in bytes {
        let mut carry = byte as u32;
        for digit in &mut digits {
            let value = *digit as u32 * 256 + carry;
            *digit = (value % 58) as u8;
            carry = value / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    std::iter::repeat_n('1', zeroes)
        .chain(
            digits
                .iter()
                .rev()
                .map(|digit| ALPHABET[*digit as usize] as char),
        )
        .collect()
}

#[tokio::test]
async fn authenticated_delivery_is_idempotent_and_writes_remote_provenance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    seed_foreman(&home, &group.group_id);
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_test","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "transport":"group_bridge_session",
                "credential":"secret-test","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_test","registration_id":"greg_test",
                "transport":"group_bridge_session","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"messages"
            }],
            "deliveries":[]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home.clone());
    let payload = json!({
        "source_group_id":"g_sender","source_group_title":"Sender",
        "source_by":"sender-agent","src_event_id":"sender-event-1",
        "idempotency_key":"delivery-1","text":"hello remote","to":["@foreman"],
        "message_mode":"send",
        "attachments":[{
            "kind":"file","title":"evidence.txt","mime_type":"text/plain",
            "bytes":8,"sha256":"remote-sha","content_base64":"ZXZpZGVuY2U="
        }]
    });

    let unauthorized = app
        .clone()
        .oneshot(request(&payload, None))
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);

    let missing_source_group = app
        .clone()
        .oneshot(request(
            &json!({
                "idempotency_key":"delivery-without-source","text":"hello remote",
                "to":["@foreman"],"message_mode":"send"
            }),
            Some("secret-test"),
        ))
        .await
        .expect("response");
    assert_eq!(missing_source_group.status(), StatusCode::FORBIDDEN);

    let missing_recipient = app
        .clone()
        .oneshot(request(
            &json!({
                "source_group_id":"g_sender","source_group_title":"Sender",
                "idempotency_key":"delivery-without-recipient","text":"hello remote","to":[],
                "message_mode":"send"
            }),
            Some("secret-test"),
        ))
        .await
        .expect("response");
    assert_eq!(missing_recipient.status(), StatusCode::BAD_REQUEST);
    let missing_recipient_body: Value = serde_json::from_slice(
        &missing_recipient
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(
        missing_recipient_body["error"]["code"],
        "missing_remote_recipient"
    );
    let unsupported_refs = app
        .clone()
        .oneshot(request(
            &json!({
                "source_group_id":"g_sender","idempotency_key":"delivery-with-refs",
                "text":"hello remote","to":["@foreman"],
                "message_mode":"send",
                "refs":[{"kind":"task_ref","task_id":"task-1"}]
            }),
            Some("secret-test"),
        ))
        .await
        .expect("response");
    assert_eq!(unsupported_refs.status(), StatusCode::BAD_REQUEST);

    for expected_deduped in [false, true] {
        let response = app
            .clone()
            .oneshot(request(&payload, Some("secret-test")))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let result: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(result["result"]["deduped"], expected_deduped);
    }

    let messages: Vec<_> = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "chat.message")
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].by, "group_bridge:peer_sender");
    assert_eq!(messages[0].data["source_group_id"], "g_sender");
    assert_eq!(messages[0].data["src_group_id"], "g_sender");
    assert_eq!(messages[0].data["src_event_id"], "sender-event-1");
    assert_eq!(messages[0].data["src_by"], "sender-agent");
    assert_eq!(messages[0].data["source_user_id"], "peer_sender");
    assert_eq!(messages[0].data["remote_reply_to"], json!(["sender-agent"]));
    assert_eq!(messages[0].data["source_platform"], "group_bridge_session");
    let attachment_path = messages[0].data["attachments"][0]["path"]
        .as_str()
        .expect("attachment path");
    assert_eq!(
        std::fs::read(
            cccc_core::blobs::resolve(&home, &group.group_id, attachment_path)
                .expect("attachment blob")
        )
        .expect("attachment bytes"),
        b"evidence"
    );
    daemon.abort();
}

#[tokio::test]
async fn authenticated_reply_request_cancellation_reaches_the_remote_message_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    seed_foreman(&home, &group.group_id);
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_cancel","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "transport":"group_bridge_session","credential":"cancel-secret","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_cancel","registration_id":"greg_cancel",
                "transport":"group_bridge_session","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"messages"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home.clone());
    let delivered = app
        .clone()
        .oneshot(request(
            &json!({
                "op":"remote_send","source_group_id":"g_sender","src_group_id":"g_sender",
                "source_by":"sender-agent","src_event_id":"sender-message-1",
                "idempotency_key":"delivery-cancel-source","text":"please answer",
                "to":["foreman"],"message_mode":"request_reply"
            }),
            Some("cancel-secret"),
        ))
        .await
        .expect("delivery response");
    let delivered_status = delivered.status();
    let delivered: Value = serde_json::from_slice(
        &delivered
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("delivery json");
    assert_eq!(delivered_status, StatusCode::OK, "{delivered}");
    let remote_source_event_id = delivered["result"]["event"]["id"]
        .as_str()
        .expect("remote source event id");
    let cancellation = json!({
        "op":"reply_request_cancel","source_group_id":"g_sender","src_group_id":"g_sender",
        "idempotency_key":"cancel-1",
        "payload":{
            "source_group_id":"g_sender",
            "source_message_event_id":"sender-message-1",
            "source_cancel_event_id":"sender-cancel-1",
            "remote_source_event_id":remote_source_event_id
        }
    });
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(request(&cancellation, Some("cancel-secret")))
            .await
            .expect("cancellation response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let cancellations = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "chat.reply_request.cancelled")
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert_eq!(
        cancellations[0].data["source_event_id"],
        remote_source_event_id
    );
    assert_eq!(cancellations[0].data["src_event_id"], "sender-cancel-1");
    daemon.abort();
}

#[tokio::test]
async fn remote_foreman_selector_fails_before_ledger_write_when_group_has_no_foreman() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.create("receiver", ""))
        .expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{"registration_id":"greg_test","transport":"group_bridge_session",
                "group_id":group.group_id,"remote_group_id":"g_sender",
                "remote_peer_id":"peer_sender","credential":"bridge-secret","status":"active"}],
            "trusts":[{"trust_id":"trust_test","registration_id":"greg_test",
                "transport":"group_bridge_session","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"messages"}],
            "deliveries":[]
        });
        Ok(())
    })
    .expect("bridge state");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(request(
            &json!({"source_group_id":"g_sender","text":"must not append",
                "to":["@foreman"],"idempotency_key":"missing-foreman",
                "message_mode":"send"}),
            Some("bridge-secret"),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let events = GroupStore::new(home)
        .and_then(|store| store.ledger_path(&group.group_id))
        .and_then(|path| ledger::read_all(&path))
        .expect("ledger");
    assert!(events.is_empty());
    daemon.abort();
}

#[tokio::test]
async fn signed_python_style_websocket_is_authorized_without_bearer_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("local")).expect("home");
    let remote_home = HomeLayout::from_path(temp.path().join("remote")).expect("remote home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("receiver", "")
        .expect("group");
    let remote_identity =
        cccc_core::group_bridge_identity::GroupBridgeIdentity::load_or_create(&remote_home)
            .expect("remote identity");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({"trusts":[{
            "trust_id":"trust_signed","registration_id":"registration_signed",
            "transport":"group_bridge_session","group_id":group.group_id.clone(),
            "remote_group_id":"g_sender","remote_peer_id":remote_identity.peer_id.clone(),
            "status":"active","access_level":"messages"
        }]});
        Ok(())
    })
    .expect("bridge state");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server =
        tokio::spawn(
            async move { axum::serve(listener, auth_support::authenticated_app(home)).await },
        );
    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws"))
            .await
            .expect("connect");
    socket
        .send(WsMessage::Text(
            remote_identity
                .sign_session_hello(&group.group_id, "g_sender")
                .expect("hello")
                .to_string()
                .into(),
        ))
        .await
        .expect("send hello");
    let ready = next_socket_json(&mut socket).await;
    assert_eq!(ready["ok"], true);
    assert_eq!(ready["type"], "ready");
    socket
        .send(WsMessage::Text(json!({"type":"ping"}).to_string().into()))
        .await
        .expect("ping");
    assert_eq!(next_socket_json(&mut socket).await["type"], "pong");
    server.abort();
}

#[tokio::test]
async fn cross_group_send_does_not_downgrade_to_legacy_remote_mcp() {
    async fn unexpected_legacy_mcp() -> StatusCode {
        panic!("current-contract delivery must not downgrade to legacy MCP")
    }

    let remote = Router::new()
        .route(
            "/api/group-bridge/session/send",
            post(|| async {
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({"detail":"loopback required"})),
                )
            }),
        )
        .route("/mcp/group-bridge", post(unexpected_legacy_mcp));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let remote_task = tokio::spawn(async move { axum::serve(listener, remote).await });

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "trusts":[{
                "trust_id":"trust_remote","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_endpoint":endpoint,
                "remote_peer_id":"12D3KooRemote","credential":"frs_test",
                "remote_access_level":"read","status":"active"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/send_cross_group",
                group.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "dst_group_id":"g_remote","text":"hello current contract",
                    "message_mode":"send",
                    "client_id":"legacy-send-1"
                })
                .to_string(),
            ))
            .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["result"]["receipt"]["transport"],
        "group_bridge_session"
    );
    assert_eq!(body["result"]["receipt"]["status"], "retrying");

    daemon.abort();
    remote_task.abort();
}

#[tokio::test]
async fn remote_mcp_reports_access_and_does_not_expose_unscoped_full_tools() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("remote target", "").expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_full","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "transport":"group_bridge_session",
                "credential":"full-token","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_full","registration_id":"greg_full",
                "transport":"group_bridge_session","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"full"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home);

    let access = app
        .clone()
        .oneshot(mcp_request("cccc_remote_access", "full-token"))
        .await
        .expect("access response");
    assert_eq!(access.status(), StatusCode::OK);
    let access: Value =
        serde_json::from_slice(&access.into_body().collect().await.expect("body").to_bytes())
            .expect("json");
    assert_eq!(
        access["result"]["structuredContent"]["permissions"]["full"],
        true
    );

    let forbidden = app
        .oneshot(mcp_request("cccc_group", "full-token"))
        .await
        .expect("forbidden response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    daemon.abort();
}

#[tokio::test]
async fn remote_exec_session_is_bound_to_the_authorized_registration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("remote target", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[
                {"registration_id":"greg_a","group_id":group.group_id,"remote_group_id":"g_sender_a","remote_peer_id":"peer_a","transport":"group_bridge_session","credential":"token-a","status":"active"},
                {"registration_id":"greg_b","group_id":group.group_id,"remote_group_id":"g_sender_b","remote_peer_id":"peer_b","transport":"group_bridge_session","credential":"token-b","status":"active"}
            ],
            "trusts":[
                {"trust_id":"trust_a","registration_id":"greg_a","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender_a","remote_peer_id":"peer_a","status":"active","access_level":"full"},
                {"trust_id":"trust_b","registration_id":"greg_b","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender_b","remote_peer_id":"peer_b","status":"active","access_level":"full"}
            ]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home);

    let started = app
        .clone()
        .oneshot(mcp_call(
            "cccc_remote_exec_command",
            "token-a",
            json!({"command":["sh","-c","sleep 30"]}),
        ))
        .await
        .expect("start response");
    assert_eq!(started.status(), StatusCode::OK);
    let started: Value = serde_json::from_slice(
        &started
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    let session_id = started["result"]["structuredContent"]["session_id"]
        .as_str()
        .expect("session id")
        .to_owned();

    let cross_registration = app
        .clone()
        .oneshot(mcp_call(
            "cccc_remote_write_stdin",
            "token-b",
            json!({"session_id":session_id}),
        ))
        .await
        .expect("cross-registration response");
    assert_eq!(cross_registration.status(), StatusCode::BAD_REQUEST);
    let denied: Value = serde_json::from_slice(
        &cross_registration
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(denied["error"]["code"], "bridge_session_not_found");

    let terminated = app
        .clone()
        .oneshot(mcp_call(
            "cccc_remote_write_stdin",
            "token-a",
            json!({"session_id":session_id,"terminate":true}),
        ))
        .await
        .expect("terminate response");
    assert_eq!(terminated.status(), StatusCode::OK);

    let after_termination = app
        .oneshot(mcp_call(
            "cccc_remote_write_stdin",
            "token-a",
            json!({"session_id":session_id}),
        ))
        .await
        .expect("post-termination response");
    assert_eq!(after_termination.status(), StatusCode::BAD_REQUEST);
    daemon.abort();
}

#[tokio::test]
async fn revoked_trust_invalidates_credential_and_removes_registration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("remote target", "")
        .expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_revoked","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "transport":"group_bridge_session",
                "credential":"revoked-token","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_revoked","registration_id":"greg_revoked",
                "transport":"group_bridge_session","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"messages"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let app = auth_support::authenticated_app(home.clone());

    let revoked = app
        .clone()
        .oneshot(
            Request::post("/api/group-bridge/pairing/trusts/trust_revoked/revoke")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"revoked_by":"test"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("revoke response");
    assert_eq!(revoked.status(), StatusCode::OK);

    let denied = app
        .oneshot(request(
            &json!({
                "source_group_id":"g_sender","text":"must fail","to":["@foreman"],
                "message_mode":"send"
            }),
            Some("revoked-token"),
        ))
        .await
        .expect("denied response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(state["trusts"][0]["status"], "revoked");
    assert_eq!(
        state["registrations"]
            .as_array()
            .expect("registrations")
            .len(),
        0
    );
}

#[tokio::test]
async fn session_authorization_requires_complete_registration_and_active_trust() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("remote target", "")
        .expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[
                {
                    "registration_id":"greg_no_trust","group_id":group.group_id,
                    "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                    "transport":"group_bridge_session",
                    "credential":"no-trust-token","status":"active"
                },
                {
                    "registration_id":"greg_incomplete","group_id":group.group_id,
                    "remote_group_id":"","remote_peer_id":"peer_sender",
                    "transport":"group_bridge_session",
                    "credential":"incomplete-token","status":"active"
                },
                {
                    "registration_id":"greg_mismatch","group_id":group.group_id,
                    "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                    "transport":"group_bridge_session",
                    "credential":"mismatch-token","status":"active"
                }
            ],
            "trusts":[
                {
                    "trust_id":"trust_incomplete","registration_id":"greg_incomplete",
                    "transport":"group_bridge_session","group_id":group.group_id,
                    "remote_group_id":"","remote_peer_id":"peer_sender",
                    "status":"active","access_level":"messages"
                },
                {
                    "trust_id":"trust_mismatch","registration_id":"greg_mismatch",
                    "transport":"group_bridge_session","group_id":group.group_id,
                    "remote_group_id":"g_sender","remote_peer_id":"different-peer",
                    "status":"active","access_level":"messages"
                }
            ]
        });
        Ok(())
    })
    .expect("bridge state");
    let app = auth_support::authenticated_app(home);

    for credential in ["no-trust-token", "incomplete-token", "mismatch-token"] {
        let response = app
            .clone()
            .oneshot(mcp_request("cccc_remote_access", credential))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn revoked_websocket_rejects_send_and_closes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("remote target", "")
        .expect("group");
    seed_active_bridge(&home, &group.group_id);
    let (server, address, mut socket) = connect_bridge_socket(home.clone()).await;
    assert_eq!(next_socket_json(&mut socket).await["type"], "ready");

    revoke_bridge(&address).await;
    socket
        .send(WsMessage::Text(
            json!({"type":"send","payload":{}}).to_string().into(),
        ))
        .await
        .expect("send");
    let error = next_socket_json(&mut socket).await;
    assert_eq!(error["type"], "error");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("no longer authorized"))
    );
    expect_socket_closed(&mut socket).await;
    server.abort();
}

#[tokio::test]
async fn revoked_websocket_polling_closes_before_forwarding_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("remote target", "").expect("group");
    seed_active_bridge(&home, &group.group_id);
    let (server, address, mut socket) = connect_bridge_socket(home.clone()).await;
    assert_eq!(next_socket_json(&mut socket).await["type"], "ready");

    revoke_bridge(&address).await;
    let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
    event.data.insert("text".into(), json!("must not escape"));
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event).expect("append");
    let error = next_socket_json(&mut socket).await;
    assert_eq!(error["type"], "error", "{error}");
    expect_socket_closed(&mut socket).await;
    server.abort();
}

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn seed_active_bridge(home: &HomeLayout, group_id: &str) {
    integration_state::global_update(home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_ws","transport":"group_bridge_session",
                "group_id":group_id,"remote_group_id":"g_sender",
                "remote_peer_id":"peer_sender","credential":"ws-token","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_ws","registration_id":"greg_ws",
                "transport":"group_bridge_session","group_id":group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "status":"active","access_level":"messages"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
}

async fn revoke_bridge(address: &str) {
    let response = reqwest::Client::new()
        .post(format!(
            "http://{address}/api/group-bridge/pairing/trusts/trust_ws/revoke"
        ))
        .bearer_auth(auth_support::TEST_ADMIN_TOKEN)
        .json(&json!({"revoked_by":"websocket-test"}))
        .send()
        .await
        .expect("revoke request");
    assert_eq!(response.status(), StatusCode::OK);
}

async fn connect_bridge_socket(
    home: HomeLayout,
) -> (
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
    String,
    TestSocket,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server =
        tokio::spawn(
            async move { axum::serve(listener, auth_support::authenticated_app(home)).await },
        );
    let mut request = format!(
        "ws://{address}/api/group-bridge/session/ws?message_contract_version={GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}"
    )
    .into_client_request()
    .expect("websocket request");
    request.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer ws-token".parse().expect("authorization"),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("connect");
    (server, address.to_string(), socket)
}

async fn next_socket_json(socket: &mut TestSocket) -> Value {
    let message = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
        .await
        .expect("websocket response timeout")
        .expect("websocket response")
        .expect("websocket message");
    serde_json::from_str(message.to_text().expect("text")).expect("json")
}

async fn expect_socket_closed(socket: &mut TestSocket) {
    let closed = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
        .await
        .expect("websocket close timeout");
    assert!(
        matches!(closed, None | Some(Ok(WsMessage::Close(_)))),
        "unexpected post-error websocket message: {closed:?}"
    );
}

fn request(payload: &Value, credential: Option<&str>) -> Request<Body> {
    let mut payload = payload.as_object().cloned().expect("payload object");
    payload.insert(
        "message_contract_version".into(),
        json!(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION),
    );
    payload.entry("op").or_insert_with(|| json!("remote_send"));
    let mut builder = Request::post("/api/group-bridge/session/send")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(credential) = credential {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {credential}"));
    }
    builder
        .body(Body::from(Value::Object(payload).to_string()))
        .expect("request")
}

fn mcp_request(tool: &str, credential: &str) -> Request<Body> {
    mcp_call(tool, credential, json!({"action":"status"}))
}

fn mcp_call(tool: &str, credential: &str, arguments: Value) -> Request<Body> {
    Request::post("/mcp/group-bridge")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::from(
            json!({
                "jsonrpc":"2.0","id":"bridge-test","method":"tools/call",
                "params":{"name":tool,"arguments":arguments}
            })
            .to_string(),
        ))
        .expect("request")
}

async fn wait_for_daemon(home: &HomeLayout) {
    let address = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if address.is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

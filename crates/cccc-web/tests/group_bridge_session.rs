use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout, Scope, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn authenticated_delivery_is_idempotent_and_writes_remote_provenance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_test","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "credential":"secret-test","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_test","registration_id":"greg_test",
                "group_id":group.group_id,"status":"active","access_level":"messages"
            }],
            "deliveries":[]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = cccc_web::app(home.clone());
    let payload = json!({
        "source_group_id":"g_sender","source_group_title":"Sender",
        "idempotency_key":"delivery-1","text":"hello remote","to":[]
    });

    let unauthorized = app
        .clone()
        .oneshot(request(&payload, None))
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);

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
    assert_eq!(messages[0].data["source_platform"], "group_bridge_session");
    daemon.abort();
}

#[tokio::test]
async fn cross_group_send_falls_back_to_remote_mcp_for_python_peers() {
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
        .route(
            "/mcp/group-bridge",
            post(|Json(request): Json<Value>| async move {
                assert_eq!(request["params"]["name"], "cccc_message_send");
                assert_eq!(request["params"]["arguments"]["text"], "hello legacy");
                Json(json!({
                    "jsonrpc":"2.0","id":request["id"],
                    "result":{"content":[{"type":"text","text":"{\"event\":{\"id\":\"remote-event\"}}"}]}
                }))
            }),
        );
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
                "remote_access_level":"messages","status":"active"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/send_cross_group",
                group.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "dst_group_id":"g_remote","text":"hello legacy",
                    "to":["@foreman"],"client_id":"legacy-send-1"
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
    assert_eq!(body["result"]["receipt"]["transport"], "group_bridge_mcp");
    assert_eq!(body["result"]["receipt"]["remote_event_id"], "remote-event");

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
                "credential":"full-token","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_full","registration_id":"greg_full",
                "group_id":group.group_id,"status":"active","access_level":"full"
            }]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = cccc_web::app(home);

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
                {"registration_id":"greg_a","group_id":group.group_id,"remote_group_id":"g_sender_a","remote_peer_id":"peer_a","credential":"token-a","status":"active"},
                {"registration_id":"greg_b","group_id":group.group_id,"remote_group_id":"g_sender_b","remote_peer_id":"peer_b","credential":"token-b","status":"active"}
            ],
            "trusts":[
                {"trust_id":"trust_a","registration_id":"greg_a","group_id":group.group_id,"status":"active","access_level":"full"},
                {"trust_id":"trust_b","registration_id":"greg_b","group_id":group.group_id,"status":"active","access_level":"full"}
            ]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = cccc_web::app(home);

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

fn request(payload: &Value, credential: Option<&str>) -> Request<Body> {
    let mut builder = Request::post("/api/group-bridge/session/send")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(credential) = credential {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {credential}"));
    }
    builder
        .body(Body::from(payload.to_string()))
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

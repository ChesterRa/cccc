use axum::body::Body;
use axum::extract::Query;
use axum::http::{Request, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Deserialize)]
struct StatusQuery {
    request_id: String,
    invite_id: String,
}

#[tokio::test]
async fn python_shaped_remote_pairing_response_becomes_active_without_claim_route() {
    let issuer = Router::new()
        .route(
            "/api/group-bridge/pairing/requests/remote",
            post(|| async {
                Json(json!({"ok":true,"result":{"request":{
                    "request_id":"preq_remote","invite_id":"pinv_remote","status":"pending"
                }}}))
            }),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote/status",
            get(|Query(query): Query<StatusQuery>| async move {
                assert_eq!(query.request_id, "preq_remote");
                assert_eq!(query.invite_id, "pinv_remote");
                Json(json!({"ok":true,"result":{"request":{
                    "request_id":"preq_remote","invite_id":"pinv_remote",
                    "registration_id":"reg_remote","status":"approved",
                    "remote_send_token":"frs_remote_token"
                }}}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let issuer_task = tokio::spawn(async move { axum::serve(listener, issuer).await });

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("joiner", "")
        .expect("group");
    let app = cccc_web::app(home.clone());
    let created = call(
        &app,
        "/api/group-bridge/pairing/remote-requests",
        json!({
            "local_group_id":group.group_id,"local_group_title":"Joiner",
            "payload":{
                "issuer_endpoint":endpoint,"issuer_group_id":"g_issuer",
                "issuer_group_title":"Issuer","issuer_peer_id":"12D3KooIssuer",
                "code":"","pairing_code":"ABCD-1234",
                "nonce":" ","invite_id":"pinv_remote"
            }
        }),
    )
    .await;
    let outbound_id = created["result"]["outbound"]["outbound_id"]
        .as_str()
        .expect("outbound id");
    assert_eq!(
        created["result"]["outbound"]["remote_request"]["request_id"],
        "preq_remote"
    );

    let synced = call(
        &app,
        &format!("/api/group-bridge/pairing/outbounds/{outbound_id}/sync"),
        json!({}),
    )
    .await;
    assert_eq!(synced["result"]["outbound"]["status"], "active");
    assert_eq!(
        synced["result"]["outbound"]["remote_request"]["request_id"],
        "preq_remote"
    );
    assert!(synced["result"]["outbound"]["remote_request"]["remote_send_token"].is_null());
    let state = integration_state::global_get(&home, "group_bridge").expect("bridge state");
    assert_eq!(state["trusts"][0]["credential"], "frs_remote_token");
    assert_eq!(
        state["trusts"][0]["trust_id"].as_str().map(str::len),
        Some(23)
    );

    issuer_task.abort();
}

async fn call(app: &Router, path: &str, body: Value) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).expect("json")
}

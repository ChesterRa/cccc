mod auth_support;
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

#[path = "group_bridge_pairing/invite_policy.rs"]
mod invite_policy;
#[path = "group_bridge_pairing/legacy_repairs.rs"]
mod legacy_repairs;
#[path = "group_bridge_pairing/remote_policy.rs"]
mod remote_policy;

#[derive(Deserialize)]
struct StatusQuery {
    request_id: String,
    invite_id: String,
}

#[tokio::test]
async fn connection_info_keeps_submitted_public_origin_in_final_payload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("issuer", "")
        .expect("group");
    std::fs::write(
        home.root().join("settings.yaml"),
        "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: http://fallback.example\n",
    )
    .expect("settings");
    let app = auth_support::authenticated_app(home);
    let created = call(
        &app,
        "/api/group-bridge/pairing/invites",
        json!({"group_id":group.group_id}),
    )
    .await;
    let invite_id = created["result"]["invite"]["invite_id"]
        .as_str()
        .expect("invite id");

    let connection_info = call(
        &app,
        "/api/group-bridge/pairing/connection-info",
        json!({
            "group_id":group.group_id,
            "invite_id":invite_id,
            "issuer_endpoint":"https://cccc.tae.vera-mesh.com/pairing?source=ui#invite",
            "issuer_group_title":"Issuer"
        }),
    )
    .await;

    assert_eq!(
        connection_info["result"]["payload"]["issuer_endpoint"],
        "https://cccc.tae.vera-mesh.com"
    );
}

#[tokio::test]
async fn remote_pairing_transport_failure_is_persisted_with_actionable_category() {
    // Port zero cannot serve a peer. Depending on OS/network policy, the
    // connection is refused or times out; both must persist an actionable error.
    let endpoint = "http://127.0.0.1:0";

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("joiner", "")
        .expect("group");
    let app = auth_support::authenticated_app(home);

    let created = call(
        &app,
        "/api/group-bridge/pairing/remote-requests",
        json!({
            "local_group_id":group.group_id,
            "local_group_title":"Joiner",
            "payload":{
                "issuer_endpoint":endpoint,
                "issuer_group_id":"g_issuer",
                "issuer_group_title":"Issuer",
                "issuer_peer_id":"12D3KooIssuer",
                "pairing_code":"ABCD-1234",
                "invite_id":"pinv_remote"
            }
        }),
    )
    .await;

    let outbound = &created["result"]["outbound"];
    assert_eq!(outbound["status"], "failed");
    let error = outbound["last_error"].as_str().expect("last error");
    assert!(
        error.contains("remote pairing request failed (connect)")
            || error.contains("remote pairing request failed (timeout after 15s)"),
        "{error}"
    );
    assert!(!error.contains("error sending request for url"));
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

async fn get_json(app: &Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
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

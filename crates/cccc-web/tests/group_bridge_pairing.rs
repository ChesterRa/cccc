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
async fn connection_info_keeps_submitted_public_origin_in_final_payload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("issuer", "")
        .expect("group");
    std::fs::write(
        home.root().join("settings.yaml"),
        "remote_access:\n  web_host: 0.0.0.0\n  web_port: 80\n  web_public_url: http://fallback.example\n",
    )
    .expect("settings");
    let app = cccc_web::app(home);
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
    // Outbound is a pairing-flow record whose terminal state is `approved` (matching the
    // Python `pairing_outbound_sync` contract). `approved` is exactly what the frontend
    // `projectRecentOutbounds` filter skips, so a completed request leaves the "sent
    // requests" list. The `active` liveness contract lives on `trust`/`registration`,
    // which must remain `active` so message routing is unaffected.
    assert_eq!(synced["result"]["outbound"]["status"], "approved");
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
    // Cross-layer contract: outbound terminal state is `approved`, but the routing trust
    // it produced stays `active` — so the pairing is done AND the session is routable.
    assert_eq!(state["outbounds"][0]["status"], "approved");
    assert_eq!(state["trusts"][0]["status"], "active");
    assert_eq!(state["trusts"][0]["transport"], "group_bridge_session");

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

/// Seeds a raw bridge store with a stale `active` outbound (the pre-fix bug shape)
/// so the read-time repair path can be exercised against persisted history.
fn seed_legacy_active_outbound(home: &cccc_core::HomeLayout, outbound: Value, trusts: Vec<Value>) {
    use cccc_core::integration_state;
    integration_state::global_update(home, "group_bridge", |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let state = value.as_object_mut().expect("bridge store initialized");
        for key in [
            "invites",
            "requests",
            "trusts",
            "registrations",
            "outbounds",
            "deliveries",
        ] {
            state.entry(key.to_owned()).or_insert_with(|| json!([]));
        }
        state
            .entry("outbounds".to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("outbounds array")
            .push(outbound);
        state
            .entry("trusts".to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("trusts array")
            .extend(trusts);
        Ok(())
    })
    .expect("seed bridge store");
}

#[tokio::test]
async fn list_repairs_legacy_active_outbound_when_matching_active_trust_exists() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("joiner", "")
        .expect("group");
    // Stale bug shape: paired outbound written as `active`, but a matching active
    // trust proves the pairing completed and routing is live.
    seed_legacy_active_outbound(
        &home,
        json!({
            "outbound_id":"pout_stale","local_group_id":group.group_id,
            "issuer_endpoint":"http://issuer","issuer_group_id":"g_issuer",
            "issuer_group_title":"Issuer","issuer_peer_id":"12D3KooIssuer",
            "status":"active","remote_request":{"request_id":"preq_stale"},
            "last_error":"","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
        }),
        vec![json!({
            "trust_id":"ptrust_stale","group_id":group.group_id,
            "remote_group_id":"g_issuer","remote_peer_id":"12D3KooIssuer",
            "transport":"group_bridge_session","status":"active","access_level":"messages"
        })],
    );

    let app = cccc_web::app(home.clone());
    let listed = get_json(
        &app,
        &format!(
            "/api/group-bridge/pairing/outbounds?group_id={}",
            group.group_id
        ),
    )
    .await;
    assert_eq!(listed["result"]["outbounds"][0]["status"], "approved");

    // Persisted once: a fresh load (no in-memory repair) still shows approved.
    let state = integration_state::global_get(&home, "group_bridge").expect("bridge state");
    assert_eq!(state["outbounds"][0]["status"], "approved");
    // The routing trust is untouched — repair only touches the outbound flow record.
    assert_eq!(state["trusts"][0]["status"], "active");
}

#[tokio::test]
async fn list_leaves_legacy_active_outbound_alone_without_matching_trust() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("joiner", "")
        .expect("group");
    // No matching trust: this `active` may be a genuine failure/orphan, so it must
    // NOT be silently hidden — preserving the audit trail for the user to inspect.
    seed_legacy_active_outbound(
        &home,
        json!({
            "outbound_id":"pout_orphan","local_group_id":group.group_id,
            "issuer_endpoint":"http://issuer","issuer_group_id":"g_issuer",
            "issuer_group_title":"Issuer","issuer_peer_id":"12D3KooIssuer",
            "status":"active","remote_request":{"request_id":"preq_orphan"},
            "last_error":"","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
        }),
        vec![],
    );

    let app = cccc_web::app(home.clone());
    let listed = get_json(
        &app,
        &format!(
            "/api/group-bridge/pairing/outbounds?group_id={}",
            group.group_id
        ),
    )
    .await;
    assert_eq!(listed["result"]["outbounds"][0]["status"], "active");
    let state = integration_state::global_get(&home, "group_bridge").expect("bridge state");
    assert_eq!(state["outbounds"][0]["status"], "active");
}

#[tokio::test]
async fn list_does_not_cross_repair_outbounds_for_different_remote_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("joiner", "")
        .expect("group");
    // Same peer, different remote group: the trust belongs to g_other, so this
    // outbound targeting g_issuer must NOT be folded — identity match is exact.
    seed_legacy_active_outbound(
        &home,
        json!({
            "outbound_id":"pout_cross","local_group_id":group.group_id,
            "issuer_endpoint":"http://issuer","issuer_group_id":"g_issuer",
            "issuer_group_title":"Issuer","issuer_peer_id":"12D3KooIssuer",
            "status":"active","remote_request":{"request_id":"preq_cross"},
            "last_error":"","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
        }),
        vec![json!({
            "trust_id":"ptrust_other","group_id":group.group_id,
            "remote_group_id":"g_other","remote_peer_id":"12D3KooIssuer",
            "transport":"group_bridge_session","status":"active","access_level":"messages"
        })],
    );

    let app = cccc_web::app(home.clone());
    let listed = get_json(
        &app,
        &format!(
            "/api/group-bridge/pairing/outbounds?group_id={}",
            group.group_id
        ),
    )
    .await;
    assert_eq!(listed["result"]["outbounds"][0]["status"], "active");
    let state = integration_state::global_get(&home, "group_bridge").expect("bridge state");
    assert_eq!(state["outbounds"][0]["status"], "active");
}

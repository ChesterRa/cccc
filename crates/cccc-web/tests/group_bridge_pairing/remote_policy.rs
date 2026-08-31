use super::*;

#[tokio::test]
async fn remote_pairing_uses_one_time_claim_route() {
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
                    "claim_available":true
                }}}))
            }),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote/claim",
            post(|| async {
                Json(json!({"ok":true,"result":{"claim":{
                    "registration_id":"reg_remote","credential":"frs_remote_token",
                    "access_level":"messages"
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
    let app = auth_support::authenticated_app(home.clone());
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
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
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
    drop(state);

    cccc_core::group_bridge_legacy::update(&home, |state| {
        state["trusts"][0]["min_session_protocol"] = json!(2);
        Ok(())
    })
    .expect("pin v2 trust");

    call(
        &app,
        &format!("/api/group-bridge/pairing/outbounds/{outbound_id}/sync"),
        json!({}),
    )
    .await;
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state after resync");
    assert_eq!(state["trusts"][0]["registration_id"], "reg_remote");
    assert_eq!(state["trusts"][0]["remote_peer_id"], "12D3KooIssuer");
    assert_eq!(state["trusts"][0]["min_session_protocol"], 2);

    issuer_task.abort();
}

#[tokio::test]
async fn remote_pairing_accepts_legacy_direct_token_without_claim_route() {
    let issuer = Router::new()
        .route(
            "/api/group-bridge/pairing/requests/remote",
            post(|| async {
                Json(json!({"ok":true,"result":{"request":{
                    "request_id":"preq_legacy","invite_id":"pinv_legacy","status":"pending"
                }}}))
            }),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote/status",
            get(|| async {
                Json(json!({"ok":true,"result":{"request":{
                    "request_id":"preq_legacy","invite_id":"pinv_legacy",
                    "registration_id":"reg_legacy","status":"approved",
                    "remote_send_token":"legacy_direct_token","access_level":"messages"
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
        .create("legacy-joiner", "")
        .expect("group");
    let app = auth_support::authenticated_app(home.clone());
    let created = call(
        &app,
        "/api/group-bridge/pairing/remote-requests",
        json!({
            "local_group_id":group.group_id,"local_group_title":"Legacy Joiner",
            "payload":{
                "issuer_endpoint":endpoint,"issuer_group_id":"g_legacy_issuer",
                "issuer_group_title":"Legacy Issuer","issuer_peer_id":"12D3KooLegacyIssuer",
                "pairing_code":"ABCD-1234","invite_id":"pinv_legacy"
            }
        }),
    )
    .await;
    let outbound_id = created["result"]["outbound"]["outbound_id"]
        .as_str()
        .expect("outbound id");

    let synced = call(
        &app,
        &format!("/api/group-bridge/pairing/outbounds/{outbound_id}/sync"),
        json!({}),
    )
    .await;

    assert_eq!(synced["result"]["outbound"]["status"], "approved");
    assert!(synced["result"]["outbound"]["remote_request"]["remote_send_token"].is_null());
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(state["trusts"][0]["registration_id"], "reg_legacy");
    assert_eq!(state["trusts"][0]["credential"], "legacy_direct_token");
    assert_eq!(state["trusts"][0]["status"], "active");

    issuer_task.abort();
}

#[tokio::test]
async fn expired_invite_is_rejected_and_persisted_as_expired() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({"invites":[{
            "invite_id":"pinv_expired","pairing_code":"ABCD-1234",
            "group_id":"g_target","status":"pending",
            "expires_at":"2020-01-01T00:00:00Z"
        }]});
        Ok(())
    })
    .expect("seed invite");
    let app = auth_support::authenticated_app(home.clone());
    let response = app
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests/remote")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "pairing_code":"ABCD-1234","invite_id":"pinv_expired",
                        "requester_group_id":"g_remote","requester_peer_id":"peer_remote",
                        "requester_endpoint":"https://remote.example"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(state["invites"][0]["status"], "expired");
}

#[tokio::test]
async fn pairing_credential_claim_is_idempotent_within_the_recovery_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "invites":[{"invite_id":"pinv_claim","pairing_code":"ABCD-1234","group_id":"g_target","status":"requested"}],
            "requests":[{"request_id":"preq_claim","invite_id":"pinv_claim","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","registration_id":"reg_claim","status":"approved","claim_expires_at":"2099-01-01T00:00:00Z","claimed_at":null}],
            "registrations":[{"registration_id":"reg_claim","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","credential":"frs_secret","status":"active"}],
            "trusts":[{"trust_id":"trust_claim","registration_id":"reg_claim","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","access_level":"messages","status":"active"}]
        });
        Ok(())
    })
    .expect("seed claim");
    let app = auth_support::authenticated_app(home);
    let status_without_invite = app
        .clone()
        .oneshot(
            Request::get("/api/group-bridge/pairing/requests/remote/status?request_id=preq_claim")
                .body(Body::empty())
                .expect("status request"),
        )
        .await
        .expect("status response");
    assert_eq!(status_without_invite.status(), StatusCode::NOT_FOUND);
    let body =
        json!({"request_id":"preq_claim","invite_id":"pinv_claim","pairing_code":"ABCD-1234"});
    let first = app
        .clone()
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests/remote/claim")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("first claim");
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store, private");
    assert_eq!(first.headers()[header::PRAGMA], "no-cache");
    let first = response_value(first).await;
    let second = app
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests/remote/claim")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("second claim");
    assert_eq!(second.status(), StatusCode::OK);
    let second = response_value(second).await;
    assert_eq!(second["result"]["claim"], first["result"]["claim"]);
}

async fn response_value(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

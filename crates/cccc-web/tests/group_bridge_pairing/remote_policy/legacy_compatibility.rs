use super::*;

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
async fn approved_legacy_claims_receive_one_persisted_upgrade_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "invites":[{"invite_id":"pinv_upgrade","pairing_code":"ABCD-1234","group_id":"g_target","status":"requested"}],
            "requests":[
                {"request_id":"preq_upgrade","invite_id":"pinv_upgrade","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","registration_id":"reg_upgrade","status":"approved","updated_at":"2026-08-01T00:00:00Z"},
                {"request_id":"preq_other","invite_id":"pinv_other","status":"approved","updated_at":"2026-08-01T00:00:00Z"}
            ],
            "registrations":[{"registration_id":"reg_upgrade","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","credential":"frs_upgrade","status":"active"}],
            "trusts":[{"trust_id":"trust_upgrade","registration_id":"reg_upgrade","group_id":"g_target","remote_group_id":"g_remote","remote_peer_id":"peer_remote","access_level":"messages","status":"active"}]
        });
        Ok(())
    })
    .expect("seed legacy approved request");
    let app = auth_support::authenticated_app(home.clone());
    let before = chrono::Utc::now();
    let status_path = "/api/group-bridge/pairing/requests/remote/status?request_id=preq_upgrade&invite_id=pinv_upgrade";

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::get(status_path)
                    .body(Body::empty())
                    .expect("status request"),
            )
            .await
            .expect("status response");
        assert_eq!(response.status(), StatusCode::OK);
    }
    let migrated = cccc_core::group_bridge_legacy::load(&home).expect("migrated bridge state");
    let requests = migrated["requests"].as_array().expect("migrated requests");
    let migrated_request = requests
        .iter()
        .find(|request| request["request_id"] == "preq_upgrade")
        .expect("migrated target request");
    let expires_at = migrated_request["claim_expires_at"]
        .as_str()
        .expect("migrated claim expiry")
        .to_owned();
    let expiry = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .expect("claim expiry")
        .with_timezone(&chrono::Utc);
    assert!(expiry > before);
    assert!(expiry <= before + chrono::Duration::minutes(11));
    assert!(migrated_request["claim_window_migrated_at"].is_string());
    assert!(
        requests
            .iter()
            .find(|request| request["request_id"] == "preq_other")
            .is_some_and(|request| request["claim_expires_at"].is_string())
    );
    drop(migrated);

    let response = app
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests/remote/claim")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "request_id":"preq_upgrade","invite_id":"pinv_upgrade",
                        "pairing_code":"ABCD-1234"
                    })
                    .to_string(),
                ))
                .expect("claim request"),
        )
        .await
        .expect("claim response");
    assert_eq!(response.status(), StatusCode::OK);
    let claimed = cccc_core::group_bridge_legacy::load(&home).expect("claimed bridge state");
    let claimed_request = claimed["requests"]
        .as_array()
        .and_then(|requests| {
            requests
                .iter()
                .find(|request| request["request_id"] == "preq_upgrade")
        })
        .expect("claimed target request");
    assert_eq!(claimed_request["claim_expires_at"], expires_at);
    assert!(claimed_request["claimed_at"].is_string());
}

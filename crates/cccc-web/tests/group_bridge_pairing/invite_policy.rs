use super::*;

#[tokio::test]
async fn same_instance_request_rejects_and_persists_an_expired_invite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let requester = GroupStore::new(home.clone())
        .expect("groups")
        .create("requester", "")
        .expect("requester");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({"invites":[{
            "invite_id":"pinv_local_expired","pairing_code":"ABCD-1234",
            "group_id":"g_target","status":"pending",
            "expires_at":"2020-01-01T00:00:00Z"
        }]});
        Ok(())
    })
    .expect("seed invite");

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "pairing_code":"ABCD-1234","invite_id":"pinv_local_expired",
                        "requester_group_id":requester.group_id,"requester_peer_id":"peer_local"
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
    assert!(state["requests"].as_array().is_none_or(Vec::is_empty));
}

#[tokio::test]
async fn same_instance_request_rejects_a_reused_invite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let requester = GroupStore::new(home.clone())
        .expect("groups")
        .create("requester", "")
        .expect("requester");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({"invites":[{
            "invite_id":"pinv_local_used","pairing_code":"ABCD-1234",
            "group_id":"g_target","status":"requested",
            "expires_at":"2099-01-01T00:00:00Z"
        }]});
        Ok(())
    })
    .expect("seed invite");

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post("/api/group-bridge/pairing/requests")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "pairing_code":"ABCD-1234","invite_id":"pinv_local_used",
                        "requester_group_id":requester.group_id,"requester_peer_id":"peer_local"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert!(state["requests"].as_array().is_none_or(Vec::is_empty));
}

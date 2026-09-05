use super::*;

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

    let app = auth_support::authenticated_app(home.clone());
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
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
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

    let app = auth_support::authenticated_app(home.clone());
    let listed = get_json(
        &app,
        &format!(
            "/api/group-bridge/pairing/outbounds?group_id={}",
            group.group_id
        ),
    )
    .await;
    assert_eq!(listed["result"]["outbounds"][0]["status"], "active");
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
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

    let app = auth_support::authenticated_app(home.clone());
    let listed = get_json(
        &app,
        &format!(
            "/api/group-bridge/pairing/outbounds?group_id={}",
            group.group_id
        ),
    )
    .await;
    assert_eq!(listed["result"]["outbounds"][0]["status"], "active");
    let state = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(state["outbounds"][0]["status"], "active");
}

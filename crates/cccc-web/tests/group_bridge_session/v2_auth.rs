use super::session_auth_support::*;
use super::*;

#[tokio::test]
async fn signed_session_disconnects_and_reconnects_without_readiness_drift() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.create("target", ""))
        .expect("group");
    seed_foreman(&home, &group.group_id);
    let signing = SigningKey::from_bytes(&[7; 32]);
    let public = signing.verifying_key().to_bytes();
    let peer_id = test_peer_id(&public);
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{"registration_id":"signed-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"credential":"unused","status":"active"}],
            "trusts":[{"trust_id":"signed-trust","registration_id":"signed-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"status":"active","access_level":"messages"}]
        });
        Ok(())
    }).expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let web_home = home.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, auth_support::authenticated_app(web_home)).await
    });

    let (mut socket, hello, challenge) =
        connect_v2_socket(&address.to_string(), &signing, &peer_id, &group.group_id).await;
    next_v2_ready(&mut socket, &hello, &challenge).await;
    wait_for_session_ready(&home, &group.group_id, &peer_id, true).await;
    complete_client_initiated_delivery(&mut socket).await;
    complete_web_delivery_over_session(&address, &home, &mut socket, &group.group_id).await;
    complete_daemon_delivery(&home, &mut socket, &group.group_id, &peer_id, "first").await;

    socket.close(None).await.expect("close");
    wait_for_session_ready(&home, &group.group_id, &peer_id, false).await;

    let (mut socket, hello, challenge) =
        connect_v2_socket(&address.to_string(), &signing, &peer_id, &group.group_id).await;
    next_v2_ready(&mut socket, &hello, &challenge).await;
    wait_for_session_ready(&home, &group.group_id, &peer_id, true).await;
    complete_daemon_delivery(&home, &mut socket, &group.group_id, &peer_id, "second").await;
    socket.close(None).await.expect("close second");
    server.abort();
    daemon.abort();
}

#[tokio::test]
async fn signed_session_hello_nonce_cannot_be_replayed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.create("target", ""))
        .expect("group");
    let signing = SigningKey::from_bytes(&[9; 32]);
    let public = signing.verifying_key().to_bytes();
    let peer_id = test_peer_id(&public);
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{"registration_id":"replay-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"credential":"unused","status":"active"}],
            "trusts":[{"trust_id":"replay-trust","registration_id":"replay-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"status":"active","access_level":"messages"}]
        });
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
    let hello = signed_hello(&signing, &peer_id, &group.group_id);

    let (mut first, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws"))
            .await
            .expect("first connect");
    first
        .send(WsMessage::Text(hello.to_string().into()))
        .await
        .expect("first hello");
    assert_eq!(next_socket_json(&mut first).await["ok"], true);
    first.close(None).await.expect("close first");

    let (mut replay, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws"))
            .await
            .expect("replay connect");
    replay
        .send(WsMessage::Text(hello.to_string().into()))
        .await
        .expect("replay hello");
    let rejected = next_socket_json(&mut replay).await;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "unauthorized_peer");
    server.abort();
}

#[tokio::test]
async fn v2_upgrade_accepts_old_client_then_pins_and_blocks_downgrade() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.create("target", ""))
        .expect("group");
    let signing = SigningKey::from_bytes(&[11; 32]);
    let peer_id = test_peer_id(&signing.verifying_key().to_bytes());
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{"registration_id":"upgrade-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"credential":"unused","status":"active"}],
            "trusts":[{"trust_id":"upgrade-trust","registration_id":"upgrade-registration","transport":"group_bridge_session","group_id":group.group_id,"remote_group_id":"g_sender","remote_peer_id":peer_id,"status":"active","access_level":"messages"}]
        });
        Ok(())
    })
    .expect("bridge state");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let web_home = home.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, auth_support::authenticated_app(web_home)).await
    });

    let (mut legacy, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws"))
            .await
            .expect("legacy connect");
    legacy
        .send(WsMessage::Text(
            legacy_signed_hello(&signing, &peer_id, &group.group_id)
                .to_string()
                .into(),
        ))
        .await
        .expect("legacy hello");
    assert_eq!(next_socket_json(&mut legacy).await["ok"], true);
    legacy.close(None).await.expect("legacy close");

    let (mut v2, captured_v2_hello, challenge) =
        connect_v2_socket(&address.to_string(), &signing, &peer_id, &group.group_id).await;
    next_v2_ready(&mut v2, &captured_v2_hello, &challenge).await;
    v2.close(None).await.expect("v2 close");
    let bridge = cccc_core::group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(bridge["trusts"][0]["min_session_protocol"], 2);

    let (mut downgrade, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws"))
            .await
            .expect("downgrade connect");
    downgrade
        .send(WsMessage::Text(
            legacy_signed_hello(&signing, &peer_id, &group.group_id)
                .to_string()
                .into(),
        ))
        .await
        .expect("downgrade hello");
    assert_eq!(next_socket_json(&mut downgrade).await["ok"], false);

    let (mut replay, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/group-bridge/session/ws/v2"))
            .await
            .expect("v2 replay connect");
    assert_eq!(next_socket_json(&mut replay).await["type"], "challenge");
    replay
        .send(WsMessage::Text(captured_v2_hello.to_string().into()))
        .await
        .expect("replayed v2 hello");
    assert_eq!(next_socket_json(&mut replay).await["ok"], false);
    server.abort();
}

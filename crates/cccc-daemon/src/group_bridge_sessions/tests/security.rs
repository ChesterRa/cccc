use super::*;

#[tokio::test]
async fn v2_client_rejects_unsigned_ready_after_a_valid_replayed_challenge() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client_identity = GroupBridgeIdentity::load_or_create(&home).expect("client identity");
    let server_identity = test_identity(&temp, "remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let config = RouteConfig {
        trust_id: "trust_ready_proof".into(),
        registration_id: "registration_ready_proof".into(),
        local_group_id: "g_local".into(),
        remote_group_id: "g_remote".into(),
        remote_peer_id: server_identity.peer_id.clone(),
        endpoint: format!("http://{}", listener.local_addr().expect("address")),
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let expected_client = client_identity.peer_id;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let _ = receive_v2_hello(&mut socket, &expected_client, &server_identity).await;
        socket
            .send(Message::Text(
                json!({
                    "ok":true,"type":"ready",
                    "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("unsigned ready");
    });

    let error = handshake::connect(&home, &config)
        .await
        .expect_err("unsigned ready must not authenticate the server");

    assert!(error.contains("ready signature is invalid"), "{error}");
    assert_eq!(persisted_minimum(&home, &config.trust_id), 1);
    server.await.expect("server");
}

#[tokio::test]
async fn successful_v2_connection_pins_and_refuses_later_v1_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client_identity = GroupBridgeIdentity::load_or_create(&home).expect("client identity");
    let server_identity = test_identity(&temp, "remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let config = RouteConfig {
        trust_id: "trust_client_pin".into(),
        registration_id: "registration_client_pin".into(),
        local_group_id: "g_local".into(),
        remote_group_id: "g_remote".into(),
        remote_peer_id: server_identity.peer_id.clone(),
        endpoint: format!("http://{}", listener.local_addr().expect("address")),
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let expected_client = client_identity.peer_id;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("v2 accept");
        let mut socket = accept_async(stream).await.expect("v2 websocket");
        let (hello, challenge) =
            receive_v2_hello(&mut socket, &expected_client, &server_identity).await;
        send_v2_ready(&mut socket, &server_identity, &hello, &challenge).await;
        drop(socket);

        let (mut probe, _) = listener.accept().await.expect("later v2 probe");
        let mut request = vec![0; 4096];
        let size = probe.read(&mut request).await.expect("read v2 probe");
        assert!(
            String::from_utf8_lossy(&request[..size])
                .contains("GET /api/group-bridge/session/ws/v2 ")
        );
        probe
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("v2 unavailable response");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_err(),
            "pinned client attempted a v1 fallback connection"
        );
    });

    drop(handshake::connect(&home, &config).await.expect("first v2"));
    assert_eq!(persisted_minimum(&home, &config.trust_id), 2);
    let error = handshake::connect(&home, &config)
        .await
        .expect_err("pinned trust must reject downgrade");

    assert!(error.contains("refusing v1 downgrade"), "{error}");
    server.await.expect("server");
}

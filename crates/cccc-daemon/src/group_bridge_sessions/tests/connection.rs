use super::*;

#[tokio::test]
async fn signed_session_registers_a_live_request_route() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let remote_identity = test_identity(&temp, "remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let expected_peer_id = identity.peer_id.clone();
    let server_identity = remote_identity.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let (hello, challenge) =
            receive_v2_hello(&mut socket, &expected_peer_id, &server_identity).await;
        assert_eq!(hello["target_group_id"], "g_remote");
        assert_eq!(hello["src_group_id"], "g_local");
        send_v2_ready(&mut socket, &server_identity, &hello, &challenge).await;
        let request = loop {
            let frame = message_json(
                socket
                    .next()
                    .await
                    .expect("request")
                    .expect("request frame"),
            )
            .expect("request json");
            if frame["type"] == "ping" {
                socket
                    .send(Message::Text(json!({"type":"pong"}).to_string().into()))
                    .await
                    .expect("pong");
                continue;
            }
            break frame;
        };
        assert_eq!(request["type"], "request");
        assert_eq!(request["op"], "remote_send");
        socket
            .send(Message::Text(
                json!({
                    "type":"response",
                    "response_to":request["request_id"],
                    "result":{"ok":true,"event_id":"remote-event"}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("response");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    let remote_peer_id = remote_identity.peer_id;
    let config = RouteConfig {
        trust_id: "trust_test".into(),
        registration_id: "registration_test".into(),
        local_group_id: "g_local".into(),
        remote_group_id: "g_remote".into(),
        remote_peer_id: remote_peer_id.clone(),
        endpoint,
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let worker_home = home.clone();
    let worker_config = config.clone();
    let worker =
        tokio::spawn(async move { connect_once(&worker_home, &worker_config, stop_rx).await });
    for _ in 0..100 {
        if route_state::contains("g_local", "g_remote", &remote_peer_id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let response = tokio::task::spawn_blocking(move || {
        send(
            "g_local",
            "g_remote",
            &remote_peer_id,
            json!({"op":"remote_send","payload":{"text":"hello"}}),
        )
    })
    .await
    .expect("send task")
    .expect("live response");
    assert_eq!(response["event_id"], "remote-event");
    let _ = stop_tx.send(true);
    let _ = worker.await.expect("worker");
    server.await.expect("server");
}

#[tokio::test]
async fn new_client_falls_back_to_v1_when_v2_endpoint_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let expected_peer_id = identity.peer_id.clone();
    let server = tokio::spawn(async move {
        let (mut probe, _) = listener.accept().await.expect("v2 probe");
        let mut request = vec![0; 4096];
        let size = probe.read(&mut request).await.expect("read v2 probe");
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(request.contains("GET /api/group-bridge/session/ws/v2 "));
        probe
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("v2 unavailable response");

        let (stream, _) = listener.accept().await.expect("v1 fallback");
        let mut socket = accept_async(stream).await.expect("v1 websocket");
        let hello = message_json(socket.next().await.expect("hello").expect("hello frame"))
            .expect("hello json");
        assert_eq!(
            authenticated_legacy_session_peer_id(&hello).as_deref(),
            Some(expected_peer_id.as_str())
        );
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
            .expect("ready");
        while socket.next().await.is_some() {}
    });
    let config = RouteConfig {
        trust_id: "trust_fallback".into(),
        registration_id: "registration_fallback".into(),
        local_group_id: "g_local_fallback".into(),
        remote_group_id: "g_remote_fallback".into(),
        remote_peer_id: "peer_remote_fallback".into(),
        endpoint,
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let worker_home = home.clone();
    let worker_config = config.clone();
    let worker =
        tokio::spawn(async move { connect_once(&worker_home, &worker_config, stop_rx).await });
    for _ in 0..100 {
        if route_state::contains(
            "g_local_fallback",
            "g_remote_fallback",
            "peer_remote_fallback",
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(route_state::contains(
        "g_local_fallback",
        "g_remote_fallback",
        "peer_remote_fallback",
    ));
    let _ = stop_tx.send(true);
    worker.await.expect("worker").expect("worker result");
    server.await.expect("server");
}

#[tokio::test]
async fn v2_client_rejects_a_server_identity_that_does_not_match_the_trust() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let expected_identity = test_identity(&temp, "expected-remote");
    let impostor_identity = test_identity(&temp, "impostor-remote");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        socket
            .send(Message::Text(
                signed_challenge(&impostor_identity).to_string().into(),
            ))
            .await
            .expect("challenge");
        let _ = socket.next().await;
    });
    let config = RouteConfig {
        trust_id: "trust_server_identity".into(),
        registration_id: "registration_server_identity".into(),
        local_group_id: "g_local".into(),
        remote_group_id: "g_remote".into(),
        remote_peer_id: expected_identity.peer_id,
        endpoint,
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let (_stop_tx, stop_rx) = watch::channel(false);

    let error = connect_once(&home, &config, stop_rx)
        .await
        .expect_err("server identity mismatch must fail");

    assert!(error.contains("identity does not match trust"), "{error}");
    server.await.expect("server");
}

use super::*;

#[tokio::test]
async fn worker_reconnects_and_projects_connection_health() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let remote_identity = test_identity(&temp, "remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let config = RouteConfig {
        trust_id: "trust_reconnect".into(),
        registration_id: "registration_reconnect".into(),
        local_group_id: "g_local".into(),
        remote_group_id: "g_remote".into(),
        remote_peer_id: remote_identity.peer_id.clone(),
        endpoint,
        min_session_protocol: 1,
    };
    group_bridge_legacy::update(&home, |state| {
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":config.trust_id.clone(),
                "registration_id":config.registration_id.clone(),
                "group_id":config.local_group_id.clone(),
                "remote_group_id":config.remote_group_id.clone(),
                "remote_peer_id":config.remote_peer_id.clone(),
                "remote_endpoint":config.endpoint.clone(),
                "transport":"group_bridge_session",
                "status":"active"
            }]),
        );
        Ok(())
    })
    .expect("state");
    let server_identity = remote_identity.clone();
    let server = tokio::spawn(async move {
        let (first, _) = listener.accept().await.expect("first accept");
        let mut first = accept_async(first).await.expect("first websocket");
        let challenge = signed_challenge(&server_identity);
        first
            .send(Message::Text(challenge.to_string().into()))
            .await
            .expect("first challenge");
        let _ = first.next().await;
        first.close(None).await.expect("first close");

        let (second, _) = listener.accept().await.expect("second accept");
        let mut second = accept_async(second).await.expect("second websocket");
        let challenge = signed_challenge(&server_identity);
        second
            .send(Message::Text(challenge.to_string().into()))
            .await
            .expect("second challenge");
        let hello = message_json(
            second
                .next()
                .await
                .expect("second hello")
                .expect("second hello frame"),
        )
        .expect("second hello json");
        send_v2_ready(&mut second, &server_identity, &hello, &challenge).await;
        while second.next().await.is_some() {}
    });
    let (stop_tx, stop_rx) = watch::channel(false);
    let worker_home = home.clone();
    let worker_config = config.clone();
    let worker = tokio::spawn(async move {
        run_worker(worker_home, worker_config, stop_rx).await;
    });
    let mut connected = false;
    for _ in 0..250 {
        let state = group_bridge_legacy::load(&home).expect("bridge state");
        if state["trusts"][0]["session_connected"] == true {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(connected, "worker did not reconnect within the test window");
    let _ = stop_tx.send(true);
    worker.await.expect("worker");
    server.await.expect("server");
    let state = group_bridge_legacy::load(&home).expect("bridge state");
    assert_eq!(state["trusts"][0]["session_connected"], false);
}

#[tokio::test]
async fn manager_drops_live_v1_route_when_trust_is_pinned_to_v2() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let config = RouteConfig {
        trust_id: "trust_live_v1_pin".into(),
        registration_id: "registration_live_v1_pin".into(),
        local_group_id: "g_local_live_v1_pin".into(),
        remote_group_id: "g_remote_live_v1_pin".into(),
        remote_peer_id: "peer_remote_live_v1_pin".into(),
        endpoint: format!("http://{}", listener.local_addr().expect("address")),
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let expected_peer_id = identity.peer_id;
    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel();
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut probe, _) = listener.accept().await.expect("v2 probe");
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
        let _ = connected_tx.send(());
        while let Some(frame) = socket.next().await {
            if matches!(frame, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
        let _ = closed_tx.send(());
    });

    let (stop_tx, stop_rx) = watch::channel(false);
    let manager_home = home.clone();
    let manager = tokio::spawn(async move {
        run_manager_with_interval(manager_home, stop_rx, Duration::from_millis(50)).await;
    });
    tokio::time::timeout(Duration::from_secs(2), connected_rx)
        .await
        .expect("v1 route did not connect")
        .expect("v1 server stopped before connection");
    for _ in 0..100 {
        if route_state::contains(
            "g_local_live_v1_pin",
            "g_remote_live_v1_pin",
            "peer_remote_live_v1_pin",
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(route_state::contains(
        "g_local_live_v1_pin",
        "g_remote_live_v1_pin",
        "peer_remote_live_v1_pin",
    ));

    group_bridge_legacy::update(&home, |state| {
        state["trusts"][0]["min_session_protocol"] = json!(2);
        Ok(())
    })
    .expect("pin trust to v2");

    tokio::time::timeout(Duration::from_secs(2), closed_rx)
        .await
        .expect("live v1 route was not closed after the v2 pin")
        .expect("v1 server stopped before observing closure");
    assert!(!route_state::contains(
        "g_local_live_v1_pin",
        "g_remote_live_v1_pin",
        "peer_remote_live_v1_pin",
    ));

    let _ = stop_tx.send(true);
    manager.await.expect("manager");
    server.await.expect("server");
}

#[tokio::test]
async fn manager_keeps_first_v2_route_after_it_pins_itself() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let client_identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let server_identity = test_identity(&temp, "self-pin-remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let config = RouteConfig {
        trust_id: "trust_self_pin".into(),
        registration_id: "registration_self_pin".into(),
        local_group_id: "g_local_self_pin".into(),
        remote_group_id: "g_remote_self_pin".into(),
        remote_peer_id: server_identity.peer_id.clone(),
        endpoint: format!("http://{}", listener.local_addr().expect("address")),
        min_session_protocol: 1,
    };
    seed_route_trust(&home, &config, 1);
    let expected_peer_id = client_identity.peer_id;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("v2 connection");
        let mut socket = accept_async(stream).await.expect("v2 websocket");
        let (hello, challenge) =
            receive_v2_hello(&mut socket, &expected_peer_id, &server_identity).await;
        send_v2_ready(&mut socket, &server_identity, &hello, &challenge).await;
        let _ = ready_tx.send(());

        tokio::select! {
            accepted = listener.accept() => {
                let _ = accepted.expect("unexpected reconnect accept");
                panic!("healthy first v2 session was reconnected after its own pin");
            }
            () = tokio::time::sleep(Duration::from_millis(400)) => {}
        }
        while socket.next().await.is_some() {}
    });

    let (stop_tx, stop_rx) = watch::channel(false);
    let manager_home = home.clone();
    let manager = tokio::spawn(async move {
        run_manager_with_interval(manager_home, stop_rx, Duration::from_millis(50)).await;
    });
    tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("v2 route did not become ready")
        .expect("v2 server stopped before readiness");
    for _ in 0..100 {
        if persisted_minimum(&home, &config.trust_id) == 2
            && route_state::contains(
                &config.local_group_id,
                &config.remote_group_id,
                &config.remote_peer_id,
            )
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(persisted_minimum(&home, &config.trust_id), 2);
    assert!(route_state::contains(
        &config.local_group_id,
        &config.remote_group_id,
        &config.remote_peer_id,
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(route_state::contains(
        &config.local_group_id,
        &config.remote_group_id,
        &config.remote_peer_id,
    ));

    let _ = stop_tx.send(true);
    manager.await.expect("manager");
    server.await.expect("server");
}

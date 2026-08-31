use super::*;

#[tokio::test]
async fn outgoing_session_resumes_a_pending_delivery_when_it_connects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.create("local", ""))
        .expect("group");
    let identity = GroupBridgeIdentity::load_or_create(&home).expect("identity");
    let remote_identity = test_identity(&temp, "remote-home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let config = RouteConfig {
        trust_id: "trust_resume".into(),
        registration_id: "registration_resume".into(),
        local_group_id: group.group_id.clone(),
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
                "transport":"group_bridge_session","status":"active",
                "remote_access_level":"messages"
            }]),
        );
        state.insert(
            "deliveries".into(),
            json!([{
                "operation":"remote_send",
                "registration_id":config.registration_id.clone(),
                "idempotency_key":"resume-on-connect","status":"retrying",
                "attempt":1,"max_attempts":5,
                "source_record_payload":{
                    "text":"resume after reconnect","to":["user"],"source_by":"user",
                    "message_mode":"send"
                },
                "payload":{
                    "text":"resume after reconnect","to":["user"],"source_by":"user",
                    "format":"plain","message_mode":"send",
                    "refs":[],"attachments":[]
                }
            }]),
        );
        Ok(())
    })
    .expect("bridge state");

    let expected_peer_id = identity.peer_id.clone();
    let server_identity = remote_identity.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let (hello, challenge) =
            receive_v2_hello(&mut socket, &expected_peer_id, &server_identity).await;
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
        assert_eq!(request["op"], "remote_send");
        assert_eq!(request["idempotency_key"], "resume-on-connect");
        socket
            .send(Message::Text(
                json!({
                    "type":"response","response_to":request["request_id"],
                    "result":{"ok":true,"receipt":{
                        "status":"sent","event_id":"remote-resumed"
                    }}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("response");
        while socket.next().await.is_some() {}
    });
    let (stop_tx, stop_rx) = watch::channel(false);
    let worker_home = home.clone();
    let worker_config = config.clone();
    let worker =
        tokio::spawn(async move { connect_once(&worker_home, &worker_config, stop_rx).await });
    let mut resumed = None;
    for _ in 0..200 {
        let state = group_bridge_legacy::load(&home).expect("bridge state");
        resumed = state["deliveries"].as_array().and_then(|receipts| {
            receipts
                .iter()
                .find(|receipt| receipt["idempotency_key"] == "resume-on-connect")
                .cloned()
        });
        if resumed
            .as_ref()
            .is_some_and(|receipt| receipt["status"] == "sent")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let resumed = resumed.expect("pending receipt");
    assert_eq!(resumed["status"], "sent", "{resumed}");
    assert_eq!(resumed["remote_event_id"], "remote-resumed");
    let _ = stop_tx.send(true);
    worker.await.expect("worker join").expect("worker result");
    server.await.expect("server");
}

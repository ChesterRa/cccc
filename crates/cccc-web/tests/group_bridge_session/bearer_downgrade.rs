use super::*;

#[tokio::test]
async fn bearer_v1_session_is_rejected_and_existing_connection_closes_after_v2_pin() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("remote target", "")
        .expect("group");
    seed_active_bridge(&home, &group.group_id);
    let (server, address, mut socket) = connect_bridge_socket(home.clone()).await;
    assert_eq!(next_socket_json(&mut socket).await["type"], "ready");

    pin_bridge_to_v2(&home);
    let error = next_socket_json(&mut socket).await;
    assert_eq!(error["type"], "error");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("no longer authorized"))
    );
    expect_socket_closed(&mut socket).await;

    let mut request = format!(
        "ws://{address}/api/group-bridge/session/ws?message_contract_version={GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}"
    )
    .into_client_request()
    .expect("websocket request");
    request.headers_mut().insert(
        header::AUTHORIZATION,
        "Bearer ws-token".parse().expect("authorization"),
    );
    let rejected = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("pinned trust must reject a new bearer v1 session");
    let status = match rejected {
        tokio_tungstenite::tungstenite::Error::Http(response) => response.status(),
        other => panic!("unexpected websocket error: {other}"),
    };
    assert_eq!(status, StatusCode::FORBIDDEN);
    server.abort();
}

fn pin_bridge_to_v2(home: &HomeLayout) {
    cccc_core::group_bridge_legacy::update(home, |state| {
        let trust = state
            .get_mut("trusts")
            .and_then(Value::as_array_mut)
            .and_then(|trusts| {
                trusts
                    .iter_mut()
                    .find(|trust| trust["trust_id"] == "trust_ws")
            })
            .ok_or_else(|| std::io::Error::other("test trust missing"))?;
        trust["min_session_protocol"] = json!(2);
        Ok(())
    })
    .expect("pin bridge protocol");
}

#[tokio::test]
async fn websocket_query_token_is_rejected_before_upgrade() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize home");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server =
        tokio::spawn(
            async move { axum::serve(listener, auth_support::authenticated_app(home)).await },
        );
    for path in ["session/ws", "session/ws/v2"] {
        let error = tokio_tungstenite::connect_async(format!(
            "ws://{address}/api/group-bridge/{path}?token=secret&message_contract_version={GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}"
        ))
        .await
        .expect_err("query token must be rejected");
        let status = match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => response.status(),
            other => panic!("unexpected websocket error: {other}"),
        };
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    server.abort();
}

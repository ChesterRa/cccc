use super::*;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

#[tokio::test]
async fn an_abandoned_correlated_start_cannot_defer_every_later_event_forever() {
    for cancelled in [false, true] {
        let (client, server) = tokio::io::duplex(4096);
        let client = tokio_tungstenite::WebSocketStream::from_raw_socket(
            client,
            tokio_tungstenite::tungstenite::protocol::Role::Client,
            None,
        )
        .await;
        let mut server = tokio_tungstenite::WebSocketStream::from_raw_socket(
            server,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        let client = std::sync::Arc::new(ProtocolClient::new(client, "fixture-start".into(), None));
        let mut events = client.subscribe();
        let request = {
            let client = std::sync::Arc::clone(&client);
            tokio::spawn(async move {
                client
                    .request(
                        "turn/start",
                        json!({
                            "threadId":"fixture-thread",
                            "responsesapiClientMetadata":{"cccc_turn_correlation_id":"fixture-input"}
                        }),
                        Duration::from_millis(100),
                    )
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), server.next())
            .await
            .expect("start request deadline")
            .expect("start request frame")
            .expect("start request");
        server
            .send(Message::Text(
                json!({"method":"turn/started","params":{"threadId":"fixture-thread","turn":{"id":"unknown-turn"}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("deferred event");
        if cancelled {
            request.abort();
            assert!(request.await.expect_err("request cancelled").is_cancelled());
        } else {
            assert_eq!(
                request
                    .await
                    .expect("request task")
                    .expect_err("unconfirmed start")
                    .kind(),
                io::ErrorKind::TimedOut
            );
        }
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("unconfirmed admission must become an explicit disconnect")
            .expect("disconnect event");
        assert_eq!(
            event.message["method"],
            super::super::MANAGED_AGENT_DISCONNECTED_METHOD
        );
        assert_eq!(
            event.message["params"]["diagnostic"]["code"],
            "admission_response_lost"
        );
        assert_eq!(
            client
                .request("thread/read", json!({}), Duration::from_millis(100))
                .await
                .expect_err("unresolved session is not reusable")
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        client.close().await;
    }
}

#[tokio::test]
async fn a_read_timeout_does_not_disconnect_a_healthy_session_or_hold_its_events() {
    let (client, server) = tokio::io::duplex(4096);
    let client = tokio_tungstenite::WebSocketStream::from_raw_socket(
        client,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        None,
    )
    .await;
    let mut server = tokio_tungstenite::WebSocketStream::from_raw_socket(
        server,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    let client = ProtocolClient::new(client, "fixture-read-timeout".into(), None);
    let mut events = client.subscribe();
    let error = client
        .request("thread/read", json!({}), Duration::from_millis(20))
        .await
        .expect_err("fixture omits read reply");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    server
        .send(Message::Text(
            json!({"method":"turn/started","params":{"threadId":"fixture-thread","turn":{"id":"later-turn"}}})
                .to_string()
                .into(),
        ))
        .await
        .expect("later event");
    let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("healthy event deadline")
        .expect("healthy event");
    assert_eq!(event.message["method"], "turn/started");
    assert_eq!(event.message["params"]["turn"]["id"], "later-turn");
    assert!(events.try_recv().is_err());
    client.close().await;
}

#[tokio::test]
async fn remote_close_records_code_without_logging_peer_reason() {
    let (client, server) = tokio::io::duplex(4096);
    let client = tokio_tungstenite::WebSocketStream::from_raw_socket(
        client,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        None,
    )
    .await;
    let mut server = tokio_tungstenite::WebSocketStream::from_raw_socket(
        server,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    let client = ProtocolClient::new(client, "fixture-generation".into(), None);
    let mut events = client.subscribe();
    server
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::Away,
            reason: "private peer reason and token".into(),
        })))
        .await
        .expect("close");
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("event timeout")
        .expect("event");
    let diagnostic = &event.message["params"]["diagnostic"];
    assert_eq!(diagnostic["code"], "remote_close");
    assert_eq!(diagnostic["close_code"], 1001);
    assert_eq!(diagnostic["stage"], "app_server");
    assert_eq!(diagnostic["generation"], "fixture-generation");
    assert!(!diagnostic.to_string().contains("private"));
    assert!(!diagnostic.to_string().contains("token"));
    client.close().await;
}

#[test]
fn io_diagnostics_preserve_os_code_without_private_error_details() {
    let error = tokio_tungstenite::tungstenite::Error::Io(io::Error::from_raw_os_error(10054));
    let diagnostic = transport_diagnostic("read_failed", &error);
    assert_eq!(diagnostic["error_kind"], "io");
    assert_eq!(diagnostic["os_error"], 10054);
    let error = tokio_tungstenite::tungstenite::Error::Io(io::Error::other("private response"));
    assert!(
        !transport_diagnostic("read_failed", &error)
            .to_string()
            .contains("private")
    );
}

#[cfg(unix)]
#[test]
fn exit_observation_precedes_cleanup_and_does_not_terminate_a_live_child() {
    use std::process::Command;
    use std::sync::Arc;
    for should_exit in [false, true] {
        let (child, tree) = cccc_runtime::OwnedProcessTree::spawn(Command::new("sh").args([
            "-c",
            if should_exit {
                "exit 23"
            } else {
                "exec sleep 60"
            },
        ]))
        .expect("fixture");
        let process = Arc::new(super::super::ChildOwner::new(child, tree));
        let started = Instant::now();
        if should_exit {
            while process.running() && started.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let diagnostic = disconnect_diagnostic(
            json!({"code":"read_failed"}),
            "fixture",
            started,
            Some(Arc::downgrade(&process)),
        );
        if should_exit {
            assert_eq!(diagnostic["process_state"], "exited");
            assert_eq!(diagnostic["exit_code"], 23);
        } else {
            assert_eq!(diagnostic["process_state"], "running");
            assert!(process.running());
        }
        process.stop().expect("cleanup");
    }
}

#[tokio::test]
async fn abrupt_peer_eof_is_distinct_from_a_close_frame_or_invalid_protocol() {
    let (client, server) = tokio::io::duplex(4096);
    let client = tokio_tungstenite::WebSocketStream::from_raw_socket(
        client,
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        None,
    )
    .await;
    let client = ProtocolClient::new(client, "fixture-eof".into(), None);
    let mut events = client.subscribe();
    drop(server);
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("disconnect deadline")
        .expect("disconnect event");
    let diagnostic = &event.message["params"]["diagnostic"];
    assert_eq!(diagnostic["code"], "read_failed");
    assert_eq!(
        diagnostic["protocol_error"],
        "reset_without_close_handshake"
    );
    let malformed = tokio_tungstenite::tungstenite::Error::Protocol(
        tokio_tungstenite::tungstenite::error::ProtocolError::InvalidOpcode(15),
    );
    assert_eq!(
        transport_diagnostic("read_failed", &malformed)["protocol_error"],
        "invalid_websocket_protocol"
    );
    client.close().await;
}

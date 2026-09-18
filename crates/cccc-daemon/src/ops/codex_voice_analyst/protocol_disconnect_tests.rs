use super::*;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

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

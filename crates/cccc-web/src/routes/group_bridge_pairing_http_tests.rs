use super::*;

#[test]
fn production_timeout_budget_covers_normal_public_latency() {
    assert_eq!(PRODUCTION_TIMEOUTS.connect, Duration::from_secs(5));
    assert_eq!(PRODUCTION_TIMEOUTS.request, Duration::from_secs(15));
}

#[test]
fn transport_failures_keep_actionable_categories() {
    assert_eq!(
        classify_failure(true, true, false, false, "TLS handshake stalled"),
        FailureKind::Timeout
    );
    assert_eq!(
        classify_failure(false, true, false, false, "dns error: no such host"),
        FailureKind::Dns
    );
    assert_eq!(
        classify_failure(false, true, false, false, "invalid peer certificate"),
        FailureKind::Tls
    );
    assert_eq!(
        classify_failure(false, true, false, false, "proxy tunnel connection failed"),
        FailureKind::Proxy
    );
    assert_eq!(
        classify_failure(false, true, false, false, "connection refused"),
        FailureKind::Connect
    );
}

#[tokio::test]
async fn stalled_peer_reports_timeout_and_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("accept");
        tokio::time::sleep(Duration::from_millis(250)).await;
    });

    let (value, error) = send_remote(
        Method::GET,
        &endpoint,
        "/slow",
        None,
        "remote pairing status",
        RemoteTimeouts {
            connect: Duration::from_millis(100),
            request: Duration::from_millis(40),
        },
    )
    .await;
    server.abort();

    assert_eq!(value, json!({}));
    assert!(error.contains("remote pairing status failed (timeout after 40ms)"));
}

#[tokio::test]
async fn unavailable_peer_reports_connect_failure() {
    // Port zero cannot host a listener. A bound-but-not-listening socket
    // can time out on macOS instead of producing a connection error.
    let endpoint = "http://127.0.0.1:0";

    let (value, error) = send_remote(
        Method::GET,
        endpoint,
        "/status",
        None,
        "remote pairing status",
        RemoteTimeouts {
            connect: Duration::from_millis(200),
            request: Duration::from_millis(400),
        },
    )
    .await;

    assert_eq!(value, json!({}));
    assert!(
        error.contains("remote pairing status failed (connect)"),
        "{error}"
    );
}

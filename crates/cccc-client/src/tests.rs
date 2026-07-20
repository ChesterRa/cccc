use super::{ClientError, DaemonClient};
use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use cccc_core::HomeLayout;
use serde_json::Map;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

async fn test_server(name: &str) -> (PathBuf, HomeLayout, TcpListener) {
    let root = std::env::temp_dir().join(format!(
        "cccc-client-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let home = HomeLayout::from_path(root.clone()).expect("home");
    home.initialize().expect("initialize");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = DaemonAddress {
        v: 1,
        transport: Transport::Tcp,
        path: String::new(),
        host: "127.0.0.1".into(),
        port: listener.local_addr().expect("address").port(),
        pid: std::process::id(),
        version: "test".into(),
        ts: "test".into(),
    };
    tokio::fs::write(
        home.daemon_dir().join("ccccd.addr.json"),
        serde_json::to_vec(&address).expect("serialize address"),
    )
    .await
    .expect("write address");
    (root, home, listener)
}

fn request(op: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: Map::new(),
    }
}

#[tokio::test]
async fn reuses_a_connection_across_calls() {
    let (root, home, listener) = test_server("reuse").await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut stream = BufReader::new(stream);
        for _ in 0..2 {
            let mut request = String::new();
            stream.read_line(&mut request).await.expect("read request");
            assert!(!request.is_empty());
            let mut payload =
                serde_json::to_vec(&DaemonResponse::success(Map::new())).expect("response");
            payload.push(b'\n');
            stream.get_mut().write_all(&payload).await.expect("write");
            stream.get_mut().flush().await.expect("flush");
        }
    });
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(2));

    assert!(client.call(&request("ping")).await.expect("first call").ok);
    assert!(client.call(&request("ping")).await.expect("second call").ok);
    server.await.expect("server");
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn reconnects_when_the_server_closed_an_idle_pooled_connection() {
    let (root, home, listener) = test_server("idle-reconnect").await;
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (first, _) = listener.accept().await.expect("first accept");
        respond_once(first).await;
        closed_tx.send(()).expect("signal close");

        let (second, _) = listener.accept().await.expect("second accept");
        respond_once(second).await;
    });
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(2));

    assert!(client.call(&request("ping")).await.expect("first call").ok);
    closed_rx.await.expect("first connection closed");
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(client.call(&request("ping")).await.expect("second call").ok);

    server.await.expect("server");
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn response_loss_does_not_replay_a_sent_request() {
    let (root, home, listener) = test_server("no-replay").await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("first accept");
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.expect("read request");
        assert!(!request.is_empty());
        drop(stream);
        tokio::time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_ok()
    });
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(2));

    assert!(matches!(
        client.call(&request("non_idempotent_write")).await,
        Err(ClientError::OutcomeUnknown { .. })
    ));
    assert!(!server.await.expect("server"), "request was replayed");
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn timeout_after_send_reports_unknown_outcome() {
    let (root, home, listener) = test_server("timeout").await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.expect("read request");
        assert!(!request.is_empty());
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let client = DaemonClient::new(home).with_timeout(Duration::from_millis(50));

    assert!(matches!(
        client.call(&request("slow_write")).await,
        Err(ClientError::OutcomeUnknown { .. })
    ));
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).expect("cleanup");
}

async fn respond_once(stream: tokio::net::TcpStream) {
    let mut stream = BufReader::new(stream);
    let mut request = String::new();
    stream.read_line(&mut request).await.expect("read request");
    assert!(!request.is_empty());
    let mut payload = serde_json::to_vec(&DaemonResponse::success(Map::new())).expect("response");
    payload.push(b'\n');
    stream.get_mut().write_all(&payload).await.expect("write");
    stream.get_mut().flush().await.expect("flush");
}

#![cfg(unix)]

use cccc_core::HomeLayout;
use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn cli_override_is_replaced_by_saved_binding_when_apply_is_requested() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let live_port = free_port().await;
    let desired_port = free_port().await;

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    command
        .arg("--port")
        .arg(live_port.to_string())
        .env("CCCC_HOME", home.root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("CCCC CLI");
    wait_for_port(live_port).await;

    let configured = request(
        live_port,
        "PUT",
        "/api/v1/remote_access",
        &json!({
            "web_host":"127.0.0.1",
            "web_port":desired_port,
            "by":"user"
        })
        .to_string(),
    )
    .await;
    assert!(configured.starts_with("HTTP/1.1 200"), "{configured}");

    let applied = request(live_port, "POST", "/api/v1/remote_access/apply", "").await;
    assert!(applied.starts_with("HTTP/1.1 200"), "{applied}");
    assert!(applied.contains(r#""accepted":true"#), "{applied}");

    wait_for_port(desired_port).await;
    wait_for_port_to_close(live_port).await;
    assert!(child.try_wait().expect("CLI status").is_none());

    let state = request(desired_port, "GET", "/api/v1/remote_access", "").await;
    assert!(state.starts_with("HTTP/1.1 200"), "{state}");
    assert!(state.contains(r#""apply_supported":true"#), "{state}");
    assert!(state.contains(r#""restart_required":false"#), "{state}");
    assert!(state.contains(&format!(r#""live_runtime_port":{desired_port}"#)));

    child.start_kill().expect("stop CLI");
    child.wait().await.expect("wait for CLI");
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind port");
    listener.local_addr().expect("address").port()
}

async fn wait_for_port(port: u16) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("Web did not listen on port {port}");
}

async fn wait_for_port_to_close(port: u16) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("old Web listener on port {port} stayed open");
}

async fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect Web");
    stream
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .expect("write request");
    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        stream.read_to_end(&mut response),
    )
    .await
    .expect("response timeout")
    .expect("read response");
    String::from_utf8(response).expect("UTF-8 response")
}

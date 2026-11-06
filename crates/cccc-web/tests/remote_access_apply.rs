#![cfg(unix)]

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Only saved: these tests end at the restart request, so nothing binds it.
const DESIRED_PORT: u16 = 18_848;

#[tokio::test]
async fn supervised_apply_returns_receipt_and_exits_for_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let admin = cccc_core::access_tokens::AccessTokenStore::new(home.clone())
        .expect("access token store")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    call(
        &home,
        "remote_access_configure",
        json!({"web_host":"127.0.0.1","web_port":DESIRED_PORT,"by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc-web"));
    command
        .env("CCCC_HOME", home.root())
        .env("CCCC_WEB_HOST", "127.0.0.1")
        .env("CCCC_WEB_PORT", "0")
        .env("CCCC_WEB_SUPERVISED", "1")
        .env_remove("CCCC_WEB_ALLOW_UNAUTHENTICATED")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().expect("web child");
    let live_port = wait_for_web_port(&home, &mut child).await;
    let response = post_apply(live_port, Some(&admin.token)).await;
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
        .await
        .expect("web child did not exit")
        .expect("web child status");
    daemon.abort();
    let _ = daemon.await;

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains(r#""accepted":true"#), "{response}");
    assert!(response.contains(&format!(
        r#""target_local_url":"http://127.0.0.1:{DESIRED_PORT}""#
    )));
    assert_eq!(status.code(), Some(75));
}

#[tokio::test]
async fn supervised_apply_rejects_remote_binding_without_admin_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    call(
        &home,
        "remote_access_configure",
        json!({"provider":"manual","web_public_url":"https://public.example","web_port":DESIRED_PORT,"by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_cccc-web"))
        .env("CCCC_HOME", home.root())
        .env("CCCC_WEB_HOST", "127.0.0.1")
        .env("CCCC_WEB_PORT", "0")
        .env("CCCC_WEB_SUPERVISED", "1")
        .env_remove("CCCC_WEB_ALLOW_UNAUTHENTICATED")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("web child");
    let live_port = wait_for_web_port(&home, &mut child).await;
    let response = post_apply(live_port, None).await;
    child.kill().await.expect("stop web child");
    let _ = child.wait().await;
    daemon.abort();
    let _ = daemon.await;

    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    assert!(
        response.contains(r#""code":"bootstrap_required""#),
        "{response}"
    );
}

fn call(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{:?}", response.error);
    Value::Object(response.result)
}

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

/// Web binds a free port itself and records it in this home. A number picked here and
/// released first can reach another test's server, or nothing, before this Web binds it.
async fn wait_for_web_port(home: &HomeLayout, web: &mut tokio::process::Child) -> u16 {
    let runtime = home.daemon_dir().join("web_runtime.json");
    // Debug builds can take several seconds to start under load.
    for _ in 0..2_000 {
        if let Ok(state) = cccc_core::fs::read_json::<Value>(&runtime)
            && let Some(port) = state["port"].as_u64().filter(|port| *port != 0)
        {
            return u16::try_from(port).expect("web port");
        }
        assert!(
            web.try_wait().expect("web status").is_none(),
            "web exited before it was ready"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("web did not report its port");
}

async fn post_apply(port: u16, token: Option<&str>) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    stream
        .write_all(
            format!(
                "POST /api/v1/remote_access/apply HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{}Content-Length: 0\r\nConnection: close\r\n\r\n",
                token.map_or_else(String::new, |value| format!("Authorization: Bearer {value}\r\n")),
            )
            .as_bytes(),
        )
        .await
        .expect("write request");
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        stream.read_to_end(&mut bytes),
    )
    .await
    .expect("response timeout")
    .expect("read response");
    String::from_utf8(bytes).expect("utf8 response")
}

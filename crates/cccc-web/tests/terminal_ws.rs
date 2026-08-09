#![cfg(unix)]

use cccc_contracts::{Actor, DaemonRequest, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, actors};
use cccc_runtime::{HistoryConfig, LaunchSpec};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn first_websocket_attach_replays_current_raw_ansi_history() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal replay", "").expect("group");
    let actor_id = "replay-peer";
    actors::add(&mut group, Actor::new(actor_id)).expect("actor");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                r"printf 'history-start\r\n'; head -c 600000 /dev/zero | tr '\0' 'x'; printf '\r\n\033[2J\033[Hcurrent screen'; sleep 5".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        HistoryConfig {
            path: temp.path().join("terminal").join("websocket.pty"),
            max_bytes: 10 * 1024 * 1024,
            hot_bytes: 10 * 1024 * 1024,
            persist: true,
        },
    )
    .expect("start terminal");
    wait_for_terminal_output(&group.group_id, actor_id).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let web_home = home.clone();
    let server = tokio::spawn(async move { axum::serve(listener, cccc_web::app(web_home)).await });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=viewer",
        group.group_id
    ))
    .await
    .expect("connect terminal websocket");

    let attach = next_binary_frame(&mut socket).await;
    assert_eq!(attach.first(), Some(&b'3'));
    let attach_payload: Value = serde_json::from_slice(&attach[1..]).expect("attach json");
    assert_eq!(attach_payload["terminal_writable"], false);
    assert_eq!(attach_payload["replay_cursor"], 0);

    let mut output = Vec::new();
    for _ in 0..20 {
        let frame = next_binary_frame(&mut socket).await;
        if frame.first() == Some(&b'1') {
            output.extend_from_slice(&frame[1..]);
        }
        if output.ends_with(b"current screen") {
            break;
        }
    }
    let output = String::from_utf8(output).expect("utf8 terminal output");
    assert!(output.len() > 512 * 1024, "replay was not paginated");
    assert!(output.contains("history-start"));
    assert!(output.contains("\u{1b}[2J\u{1b}[H"));
    assert!(output.contains("current screen"));

    let _ = socket.send(Message::Close(None)).await;
    cccc_runtime::stop(&group.group_id, actor_id).expect("stop terminal");
    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
    server.abort();
}

#[tokio::test]
async fn high_volume_initial_replay_does_not_starve_control_input() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal control", "").expect("group");
    let actor_id = "control-peer";
    actors::add(&mut group, Actor::new(actor_id)).expect("actor");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    cccc_runtime::start_with_history(
        LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                "exec yes 012345678901234567890123456789".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        HistoryConfig {
            path: temp.path().join("terminal").join("control.pty"),
            max_bytes: 2 * 1024 * 1024,
            hot_bytes: 2 * 1024 * 1024,
            persist: false,
        },
    )
    .expect("start terminal");
    wait_for_terminal_bytes(&group.group_id, actor_id, 1024 * 1024).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let web_home = home.clone();
    let server = tokio::spawn(async move { axum::serve(listener, cccc_web::app(web_home)).await });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=control",
        group.group_id
    ))
    .await
    .expect("connect terminal websocket");

    let attach = next_binary_frame(&mut socket).await;
    assert_eq!(attach.first(), Some(&b'3'));
    socket
        .send(Message::Binary(vec![b'0', 3].into()))
        .await
        .expect("queue ctrl-c");

    let stopped = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let _ = tokio::time::timeout(Duration::from_millis(50), socket.next()).await;
            let stopped = match cccc_runtime::status(&group.group_id, actor_id) {
                Ok(status) => !status.running,
                Err(cccc_runtime::RuntimeError::NotFound(_, _)) => true,
                Err(_) => false,
            };
            if stopped {
                break;
            }
        }
    })
    .await
    .is_ok();

    let _ = socket.send(Message::Close(None)).await;
    let _ = cccc_runtime::stop(&group.group_id, actor_id);
    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
    server.abort();
    assert!(stopped, "Ctrl-C was starved by the initial replay loop");
}

async fn next_binary_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Vec<u8> {
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("terminal websocket timeout")
        .expect("terminal websocket closed")
        .expect("terminal websocket message");
    match message {
        Message::Binary(data) => data.to_vec(),
        other => panic!("expected binary terminal frame, got {other:?}"),
    }
}

async fn wait_for_terminal_output(group_id: &str, actor_id: &str) {
    for _ in 0..100 {
        if cccc_runtime::retained_history(group_id, actor_id)
            .is_ok_and(|page| page.data.contains("current screen"))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("terminal output did not become ready");
}

async fn wait_for_terminal_bytes(group_id: &str, actor_id: &str, minimum: u64) {
    for _ in 0..200 {
        if cccc_runtime::retained_history(group_id, actor_id)
            .is_ok_and(|page| page.end_cursor.saturating_sub(page.start_cursor) >= minimum)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("terminal output did not reach {minimum} retained bytes");
}

async fn wait_for_daemon(home: &HomeLayout) {
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client.call(&request("ping")).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

async fn shutdown_daemon(home: &HomeLayout) {
    cccc_client::DaemonClient::new(home.clone())
        .call(&request("shutdown"))
        .await
        .expect("shutdown daemon");
}

fn request(op: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: Map::new(),
    }
}

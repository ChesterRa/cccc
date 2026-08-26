#![cfg(unix)]
mod auth_support;

use cccc_contracts::{Actor, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, actors};
use cccc_runtime::{HistoryConfig, LaunchSpec};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[path = "support/terminal_ws.rs"]
mod support;
use support::*;

#[tokio::test]
async fn websocket_attach_streams_full_replay_and_keeps_legacy_clients_compatible() {
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
    let server = tokio::spawn(async move {
        axum::serve(listener, auth_support::authenticated_app(web_home)).await
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=viewer&output_flow=ack_v1",
        group.group_id
    ))
    .await
    .expect("connect terminal websocket");

    let attach = next_binary_frame(&mut socket).await;
    assert_eq!(attach.first(), Some(&b'3'));
    let attach_payload: Value = serde_json::from_slice(&attach[1..]).expect("attach json");
    assert_eq!(attach_payload["terminal_writable"], false);
    assert_eq!(attach_payload["terminal_response_owner"], "server_v1");
    assert_eq!(attach_payload["output_flow_control"]["protocol"], "ack_v1");
    assert_eq!(
        attach_payload["output_flow_control"]["window_bytes"],
        256 * 1024
    );
    assert_eq!(attach_payload["replay_cursor"], 0);
    let replay_end_cursor = attach_payload["replay_end_cursor"]
        .as_u64()
        .expect("replay end cursor");
    assert!(replay_end_cursor > 512 * 1024);

    let (output, consumed_cursor) = read_replay(
        &mut socket,
        attach_payload["replay_cursor"].as_u64().unwrap_or(0),
        true,
    )
    .await;
    let output = String::from_utf8(output).expect("utf8 terminal output");
    assert!(
        output.len() > 512 * 1024,
        "full replay was truncated at {} bytes",
        output.len()
    );
    assert!(output.contains("history-start"));
    assert!(output.contains("\u{1b}[2J\u{1b}[H"));
    assert!(output.contains("current screen"));
    assert_eq!(consumed_cursor, replay_end_cursor);

    let _ = socket.send(Message::Close(None)).await;

    let (mut legacy_socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=viewer",
        group.group_id
    ))
    .await
    .expect("connect legacy terminal websocket");
    let legacy_attach = next_binary_frame(&mut legacy_socket).await;
    let legacy_payload: Value =
        serde_json::from_slice(&legacy_attach[1..]).expect("legacy attach json");
    assert!(legacy_payload.get("output_flow_control").is_none());
    let (legacy_output, legacy_cursor) = read_replay(
        &mut legacy_socket,
        legacy_payload["replay_cursor"].as_u64().unwrap_or(0),
        false,
    )
    .await;
    assert!(legacy_output.ends_with(b"current screen"));
    assert_eq!(legacy_cursor, legacy_payload["replay_end_cursor"]);
    let _ = legacy_socket.send(Message::Close(None)).await;

    let control_url = format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=control&since={replay_end_cursor}",
        group.group_id
    );
    let (mut first_control, _) = tokio_tungstenite::connect_async(&control_url)
        .await
        .expect("connect first control");
    let first_control_attach = read_attach_payload(&mut first_control).await;
    assert_eq!(first_control_attach["terminal_writable"], true);

    let (mut second_control, _) = tokio_tungstenite::connect_async(&control_url)
        .await
        .expect("connect second control");
    let second_control_attach = read_attach_payload(&mut second_control).await;
    assert_eq!(second_control_attach["terminal_writable"], false);

    let (mut takeover, _) =
        tokio_tungstenite::connect_async(format!("{control_url}&takeover=true"))
            .await
            .expect("connect takeover control");
    let takeover_attach = read_attach_payload(&mut takeover).await;
    assert_eq!(takeover_attach["terminal_writable"], true);
    assert_eq!(takeover_attach["writer_replaced"], true);
    assert!(!read_writable_payload(&mut first_control).await);
    let _ = takeover.send(Message::Close(None)).await;
    assert!(read_writable_payload(&mut first_control).await);
    let _ = second_control.send(Message::Close(None)).await;
    let _ = first_control.send(Message::Close(None)).await;

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
    let server = tokio::spawn(async move {
        axum::serve(listener, auth_support::authenticated_app(web_home)).await
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=control&output_flow=ack_v1",
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

    let stopped = tokio::time::timeout(Duration::from_secs(2), async {
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

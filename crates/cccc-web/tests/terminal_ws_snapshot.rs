#![cfg(unix)]
mod auth_support;

use cccc_contracts::{Actor, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, actors};
use cccc_runtime::{HistoryConfig, LaunchSpec};
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_tungstenite::tungstenite::Message;

#[path = "support/terminal_ws.rs"]
#[allow(dead_code)]
mod support;
use support::*;

#[tokio::test]
async fn negotiated_snapshot_opens_at_the_latest_screen_and_fences_live_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("terminal snapshot", "").expect("group");
    let actor_id = "snapshot-peer";
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
                "printf 'history-start\\r\\n'; yes 'old retained line' | head -n 12000; printf '\\033[2J\\033[Hcurrent screen'; IFS= read -r line; printf ' live-tail:%s' \"$line\"; sleep 5".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        HistoryConfig {
            path: temp.path().join("terminal").join("snapshot.pty"),
            max_bytes: 4 * 1024 * 1024,
            hot_bytes: 4 * 1024 * 1024,
            persist: false,
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
        "ws://{address}/api/v1/groups/{}/actors/{actor_id}/term?mode=control&output_flow=ack_v1&bootstrap=snapshot_v1&cols=80&rows=24",
        group.group_id
    ))
    .await
    .expect("connect terminal websocket");

    let attach = next_binary_frame(&mut socket).await;
    assert_eq!(attach.first(), Some(&b'3'));
    let attach: Value = serde_json::from_slice(&attach[1..]).expect("attach json");
    let cursor = attach["replay_cursor"].as_u64().expect("snapshot cursor");
    assert_eq!(attach["replay_end_cursor"], cursor);
    assert_eq!(attach["initial_output"]["kind"], "snapshot");
    assert_eq!(attach["initial_output"]["cursor"], cursor);
    assert_eq!(attach["initial_output"]["cols"], 80);
    assert_eq!(attach["initial_output"]["rows"], 24);

    let snapshot = next_binary_frame(&mut socket).await;
    assert_eq!(snapshot.first(), Some(&b'7'));
    assert_eq!(
        snapshot.len() - 1,
        attach["initial_output"]["bytes"]
            .as_u64()
            .expect("snapshot bytes") as usize
    );
    let rendered = String::from_utf8_lossy(&snapshot[1..]);
    assert!(rendered.contains("current screen"));
    assert!(!rendered.contains("history-start"));
    assert!(snapshot.len() < cursor as usize);

    let mut acknowledgement = vec![b'5'];
    acknowledgement.extend_from_slice(json!({"cursor":cursor}).to_string().as_bytes());
    socket
        .send(Message::Binary(acknowledgement.into()))
        .await
        .expect("ack snapshot");

    socket
        .send(Message::Binary(b"0go\n".to_vec().into()))
        .await
        .expect("send live input");
    let mut live_output = Vec::new();
    for _ in 0..8 {
        let live = next_binary_frame(&mut socket).await;
        if live.first() == Some(&b'1') {
            live_output.extend_from_slice(&live[1..]);
        }
        if live_output
            .windows(b"live-tail:go".len())
            .any(|window| window == b"live-tail:go")
        {
            break;
        }
    }
    assert!(
        live_output
            .windows(b"live-tail:go".len())
            .any(|window| window == b"live-tail:go")
    );

    let _ = socket.send(Message::Close(None)).await;
    cccc_runtime::stop(&group.group_id, actor_id).expect("stop terminal");
    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
    server.abort();
}

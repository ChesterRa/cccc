use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn output_ack(cursor: u64) -> Message {
    let mut frame = vec![b'5'];
    frame.extend_from_slice(json!({"cursor":cursor}).to_string().as_bytes());
    Message::Binary(frame.into())
}

pub async fn read_replay(
    socket: &mut TestSocket,
    mut cursor: u64,
    acknowledge: bool,
) -> (Vec<u8>, u64) {
    let mut output = Vec::new();
    for _ in 0..200 {
        let frame = next_binary_frame(socket).await;
        if frame.first() == Some(&b'1') {
            output.extend_from_slice(&frame[1..]);
            cursor = cursor.saturating_add((frame.len() - 1) as u64);
            if acknowledge {
                socket
                    .send(output_ack(cursor))
                    .await
                    .expect("ack terminal output");
            }
        }
        if output.ends_with(b"current screen") {
            break;
        }
    }
    (output, cursor)
}

pub async fn next_binary_frame(socket: &mut TestSocket) -> Vec<u8> {
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

pub async fn read_attach_payload(socket: &mut TestSocket) -> Value {
    let frame = next_binary_frame(socket).await;
    assert_eq!(frame.first(), Some(&b'3'));
    serde_json::from_slice(&frame[1..]).expect("attach payload")
}

pub async fn read_writable_payload(socket: &mut TestSocket) -> bool {
    loop {
        let frame = next_binary_frame(socket).await;
        if frame.first() != Some(&b'6') {
            continue;
        }
        let payload: Value = serde_json::from_slice(&frame[1..]).expect("writable payload");
        return payload["terminal_writable"]
            .as_bool()
            .expect("terminal_writable bool");
    }
}

pub fn resize_frame(cols: u64, rows: u64) -> Message {
    let mut frame = vec![b'2'];
    frame.extend_from_slice(json!({"cols":cols,"rows":rows}).to_string().as_bytes());
    Message::Binary(frame.into())
}

/// Reads size frames until the PTY size `expected` arrives, and returns it.
pub async fn read_size_payload(socket: &mut TestSocket, expected: (u64, u64)) -> (u64, u64) {
    for _ in 0..50 {
        let frame = next_binary_frame(socket).await;
        if frame.first() != Some(&b'2') {
            continue;
        }
        let payload: Value = serde_json::from_slice(&frame[1..]).expect("size payload");
        let size = (
            payload["cols"].as_u64().expect("cols"),
            payload["rows"].as_u64().expect("rows"),
        );
        if size == expected {
            return size;
        }
    }
    panic!("the terminal never reported its size as {expected:?}");
}

pub async fn daemon_call(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await
        .expect("daemon call");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

/// Waits for output from a terminal the daemon started, wherever its PTY lives.
pub async fn wait_for_daemon_terminal_output(home: &HomeLayout, group_id: &str, actor_id: &str) {
    for _ in 0..200 {
        let tail = daemon_call(
            home,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        )
        .await;
        if tail["text"]
            .as_str()
            .is_some_and(|text| text.contains("current screen"))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("terminal output did not become ready");
}

pub async fn wait_for_terminal_output(group_id: &str, actor_id: &str) {
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

pub async fn wait_for_terminal_bytes(group_id: &str, actor_id: &str, minimum: u64) {
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

pub async fn wait_for_daemon(home: &HomeLayout) {
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client.call(&request("ping")).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

pub async fn shutdown_daemon(home: &HomeLayout) {
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

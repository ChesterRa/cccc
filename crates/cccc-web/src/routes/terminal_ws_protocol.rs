use axum::extract::ws::{Message, WebSocket};
use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};

use crate::AppState;

pub(super) async fn handle_input(
    socket: &mut WebSocket,
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    writable: bool,
    message: Message,
) -> bool {
    let Message::Binary(data) = message else {
        return !matches!(message, Message::Close(_));
    };
    let Some((&opcode, payload)) = data.split_first() else {
        return true;
    };
    match opcode {
        b'0' if writable => {
            let response = daemon_call(
                state,
                "terminal_write",
                json!({"group_id":group_id,"actor_id":actor_id,"data":String::from_utf8_lossy(payload)}),
            )
            .await;
            match response {
                Some(response) if response.ok => true,
                Some(response) => {
                    let error = response.error.map_or_else(
                        || {
                            (
                                "write_failed".into(),
                                "Failed to write terminal input.".into(),
                            )
                        },
                        |error| (error.code, error.message),
                    );
                    send_input_error(socket, &error.0, &error.1).await
                }
                None => {
                    send_input_error(
                        socket,
                        "daemon_unavailable",
                        "Terminal service is unavailable.",
                    )
                    .await
                }
            }
        }
        b'0' => {
            send_input_error(
                socket,
                "viewer_only",
                "This terminal connection is read-only; reconnect as control to write.",
            )
            .await
        }
        b'2' if writable => {
            let size: Value = serde_json::from_slice(payload).unwrap_or_else(|_| json!({}));
            daemon_call(
                state,
                "terminal_resize",
                json!({"group_id":group_id,"actor_id":actor_id,"cols":size.get("cols"),"rows":size.get("rows")}),
            )
            .await
            .is_some_and(|response| response.ok)
        }
        _ => true,
    }
}

async fn send_input_error(socket: &mut WebSocket, code: &str, message: &str) -> bool {
    let payload = json!({
        "type":"terminal.input_ack",
        "ok":false,
        "error":{"code":code,"message":message},
    });
    socket
        .send(Message::Binary(
            frame(b'4', payload.to_string().as_bytes()).into(),
        ))
        .await
        .is_ok()
}

pub(super) async fn daemon_call(
    state: &AppState,
    op: &str,
    args: Value,
) -> Option<cccc_contracts::DaemonResponse> {
    state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        })
        .await
        .ok()
}

pub(super) fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(opcode);
    frame.extend_from_slice(payload);
    frame
}

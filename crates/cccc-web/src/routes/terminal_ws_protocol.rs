use axum::extract::ws::{Message, WebSocket};
use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::terminal_ws_flow::OutputFlow;
use crate::AppState;

pub(super) struct TerminalInputContext<'a> {
    pub(super) state: &'a AppState,
    pub(super) group_id: &'a str,
    pub(super) actor_id: &'a str,
    pub(super) attachment_id: u64,
    pub(super) writable: bool,
}

pub(super) async fn handle_stream_input<W>(
    socket: &mut WebSocket,
    context: TerminalInputContext<'_>,
    message: Message,
    daemon: &mut W,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    let Message::Binary(data) = message else {
        return !matches!(message, Message::Close(_));
    };
    let Some((&opcode, payload)) = data.split_first() else {
        return true;
    };
    match opcode {
        b'0' if context.writable => {
            if payload.is_empty() {
                return true;
            }
            if daemon.write_all(payload).await.is_err() || daemon.flush().await.is_err() {
                let _ = send_input_error(socket, "write_failed", "Failed to write terminal input.")
                    .await;
                return false;
            }
            true
        }
        b'0' => {
            send_input_error(
                socket,
                "viewer_only",
                "This terminal connection is read-only; reconnect as control to write.",
            )
            .await
        }
        b'2' if context.writable => {
            let size: Value = serde_json::from_slice(payload).unwrap_or_else(|_| json!({}));
            daemon_call(
                context.state,
                "term_resize",
                json!({
                    "group_id":context.group_id,
                    "actor_id":context.actor_id,
                    "attachment_id":context.attachment_id,
                    "cols":size.get("cols"),
                    "rows":size.get("rows")
                }),
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

pub(super) async fn terminal_writable(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    attachment_id: u64,
) -> Option<bool> {
    daemon_call(
        state,
        "term_attachment_status",
        json!({
            "group_id": group_id,
            "actor_id": actor_id,
            "attachment_id": attachment_id,
        }),
    )
    .await
    .filter(|response| response.ok)
    .and_then(|response| {
        response
            .result
            .get("terminal_writable")
            .and_then(Value::as_bool)
    })
}

pub(super) async fn send_output_frame(
    socket: &mut WebSocket,
    opcode: u8,
    data: &[u8],
    end_cursor: u64,
    flow: &mut OutputFlow,
) -> bool {
    if data.is_empty() {
        return true;
    }
    if socket
        .send(Message::Binary(frame(opcode, data).into()))
        .await
        .is_err()
    {
        return false;
    }
    flow.record_send(end_cursor, data.len());
    true
}

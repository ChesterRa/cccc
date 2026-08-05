use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use cccc_contracts::DaemonRequest;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;

const MAX_CONSECUTIVE_POLL_FAILURES: usize = 20;
const TERMINAL_POLL_LIMIT_BYTES: usize = 64_000;

#[derive(Debug, Deserialize)]
struct AttachQuery {
    #[serde(default = "control")]
    mode: String,
    since: Option<u64>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/groups/{group_id}/actors/{actor_id}/term",
        get(upgrade),
    )
}

async fn upgrade(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Query(query): Query<AttachQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if terminal_disabled(state.web_mode, state.exhibit_allow_terminal) {
        return ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_terminal",
                "Terminal is disabled in read-only (exhibit) mode.",
            )
            .await;
        });
    }
    ws.on_upgrade(move |socket| serve(socket, state, group_id, actor_id, query))
}

async fn serve(
    mut socket: WebSocket,
    state: AppState,
    group_id: String,
    actor_id: String,
    query: AttachQuery,
) {
    let requested_cursor = query.since.unwrap_or(0);
    let writable = terminal_writable(state.web_mode, &query.mode);
    let status = daemon_call(
        &state,
        "terminal_status",
        json!({"group_id":group_id,"actor_id":actor_id}),
    )
    .await;
    if !status.as_ref().is_some_and(|response| response.ok) {
        let error = status.and_then(|response| response.error).map_or_else(
            || json!({"code":"actor_not_running","message":"actor is not running"}),
            |error| json!({"code":error.code,"message":error.message}),
        );
        let _ = socket
            .send(Message::Text(
                json!({"ok":false,"error":error}).to_string().into(),
            ))
            .await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let Some(initial) = poll_output(&state, &group_id, &actor_id, requested_cursor).await else {
        send_terminal_error(
            &mut socket,
            "daemon_unavailable",
            "Terminal output is temporarily unavailable.",
        )
        .await;
        return;
    };
    let attach = frame(
        b'3',
        json!({"terminal_writable":writable,"replay_cursor":initial.replay_cursor})
            .to_string()
            .as_bytes(),
    );
    if socket.send(Message::Binary(attach.into())).await.is_err() {
        return;
    }
    if !initial.data.is_empty()
        && socket
            .send(Message::Binary(frame(b'1', &initial.data).into()))
            .await
            .is_err()
    {
        return;
    }
    let mut cursor = initial.next_cursor;
    let mut consecutive_poll_failures = 0;
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut shutdown = state.shutdown.subscribe();
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            _ = interval.tick() => {
                let Some(output) = poll_output(&state, &group_id, &actor_id, cursor).await else {
                    consecutive_poll_failures += 1;
                    if consecutive_poll_failures < MAX_CONSECUTIVE_POLL_FAILURES {
                        continue;
                    }
                    tracing::warn!(
                        %group_id,
                        %actor_id,
                        cursor,
                        "terminal websocket polling failed repeatedly"
                    );
                    send_terminal_error(
                        &mut socket,
                        "daemon_unavailable",
                        "Terminal output connection was interrupted.",
                    )
                    .await;
                    break;
                };
                consecutive_poll_failures = 0;
                cursor = output.next_cursor;
                if !output.data.is_empty() && socket.send(Message::Binary(frame(b'1', &output.data).into())).await.is_err() {
                    break;
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break; };
                if !handle_input(&mut socket, &state, &group_id, &actor_id, writable, message).await {
                    break;
                }
            }
        }
    }
}

fn terminal_disabled(web_mode: crate::WebMode, exhibit_allow_terminal: bool) -> bool {
    web_mode.is_read_only() && !exhibit_allow_terminal
}

fn terminal_writable(web_mode: crate::WebMode, requested_mode: &str) -> bool {
    !web_mode.is_read_only() && requested_mode != "viewer"
}

struct PolledOutput {
    data: Vec<u8>,
    replay_cursor: u64,
    next_cursor: u64,
}

async fn poll_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    cursor: u64,
) -> Option<PolledOutput> {
    let response = daemon_call(
        state,
        "terminal_since",
        json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "after":cursor,
            "limit_bytes":TERMINAL_POLL_LIMIT_BYTES,
        }),
    )
    .await?;
    if !response.ok {
        return None;
    }
    let history = response.result.get("history")?;
    let data = history.get("data")?.as_str()?.as_bytes();
    let start = history.get("start_cursor")?.as_u64()?;
    let end = history.get("end_cursor")?.as_u64()?;
    Some(PolledOutput {
        data: data.to_vec(),
        replay_cursor: start,
        next_cursor: end,
    })
}

async fn send_terminal_error(socket: &mut WebSocket, code: &str, message: &str) {
    let _ = socket
        .send(Message::Text(
            json!({"ok":false,"error":{"code":code,"message":message}})
                .to_string()
                .into(),
        ))
        .await;
    let _ = socket.send(Message::Close(None)).await;
}

async fn handle_input(
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

async fn daemon_call(
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

fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(opcode);
    frame.extend_from_slice(payload);
    frame
}

fn control() -> String {
    "control".into()
}

#[cfg(test)]
mod tests {
    use super::{terminal_disabled, terminal_writable};
    use crate::WebMode;

    #[test]
    fn exhibit_terminal_is_disabled_by_default_and_never_writable() {
        assert!(terminal_disabled(WebMode::Exhibit, false));
        assert!(!terminal_disabled(WebMode::Exhibit, true));
        assert!(!terminal_writable(WebMode::Exhibit, "control"));
        assert!(terminal_writable(WebMode::Normal, "control"));
        assert!(!terminal_writable(WebMode::Normal, "viewer"));
    }
}

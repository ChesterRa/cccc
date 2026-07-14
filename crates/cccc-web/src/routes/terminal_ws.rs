use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use cccc_contracts::DaemonRequest;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;

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
    ws.on_upgrade(move |socket| serve(socket, state, group_id, actor_id, query))
}

async fn serve(
    mut socket: WebSocket,
    state: AppState,
    group_id: String,
    actor_id: String,
    query: AttachQuery,
) {
    let mut cursor = query.since.unwrap_or(0);
    let writable = query.mode != "viewer";
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
    let attach = frame(
        b'3',
        json!({"terminal_writable":writable,"replay_cursor":cursor})
            .to_string()
            .as_bytes(),
    );
    if socket.send(Message::Binary(attach.into())).await.is_err() {
        return;
    }
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let Some((data, next)) = poll_output(&state, &group_id, &actor_id, cursor).await else {
                    break;
                };
                cursor = next;
                if !data.is_empty() && socket.send(Message::Binary(frame(b'1', &data).into())).await.is_err() {
                    break;
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break; };
                if !handle_input(&state, &group_id, &actor_id, writable, message).await {
                    break;
                }
            }
        }
    }
}

async fn poll_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    cursor: u64,
) -> Option<(Vec<u8>, u64)> {
    let response = daemon_call(
        state,
        "terminal_history",
        json!({"group_id":group_id,"actor_id":actor_id,"limit_bytes":2_000_000}),
    )
    .await?;
    if !response.ok {
        return None;
    }
    let history = response.result.get("history")?;
    let data = history.get("data")?.as_str()?.as_bytes();
    let start = history.get("start_cursor")?.as_u64()?;
    let end = history.get("end_cursor")?.as_u64()?;
    let offset = usize::try_from(cursor.saturating_sub(start)).ok()?;
    let unread = if cursor < start {
        data
    } else {
        data.get(offset..).unwrap_or(data)
    };
    Some((unread.to_vec(), end))
}

async fn handle_input(
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
        b'0' if writable => daemon_call(
            state,
            "terminal_write",
            json!({"group_id":group_id,"actor_id":actor_id,"data":String::from_utf8_lossy(payload)}),
        )
        .await
        .is_some_and(|response| response.ok),
        b'0' => true,
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

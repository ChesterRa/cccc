use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use serde::Deserialize;
use serde_json::json;

use super::terminal_ws_protocol::{daemon_call, frame, handle_input};
use super::terminal_ws_replay::{initial_output, poll_output, replay_output};
use crate::AppState;

const MAX_CONSECUTIVE_POLL_FAILURES: usize = 20;

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
    let writable = terminal_writable(state.web_mode, &query.mode);
    let status = daemon_call(
        &state,
        "terminal_status",
        json!({"group_id":group_id,"actor_id":actor_id}),
    )
    .await;
    if !status.as_ref().is_some_and(|response| {
        response.ok
            && response
                .result
                .get("session")
                .and_then(|session| session.get("running"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    }) {
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
    let Some(initial) = initial_output(&state, &group_id, &actor_id, query.since).await else {
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
    let replay_end_cursor = initial.replay_end_cursor;
    while cursor < replay_end_cursor {
        let Some(output) = replay_output(
            &state,
            &group_id,
            &actor_id,
            Some(cursor),
            Some(replay_end_cursor),
        )
        .await
        else {
            send_terminal_error(
                &mut socket,
                "daemon_unavailable",
                "Terminal history replay was interrupted.",
            )
            .await;
            return;
        };
        if output.next_cursor <= cursor {
            break;
        }
        cursor = output.next_cursor;
        if !output.data.is_empty()
            && socket
                .send(Message::Binary(frame(b'1', &output.data).into()))
                .await
                .is_err()
        {
            return;
        }
    }
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

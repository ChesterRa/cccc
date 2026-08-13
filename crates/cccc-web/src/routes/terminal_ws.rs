use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::time::Duration;
use tokio::io::AsyncReadExt;

use super::terminal_ws_bootstrap::{SNAPSHOT_V1, read_snapshot, snapshot_bootstrap};
use super::terminal_ws_flow::{OutputFlow, output_ack_cursor};
use super::terminal_ws_protocol::{
    TerminalInputContext, frame, handle_stream_input, send_output_frame, terminal_writable,
};
use crate::AppState;

const TERMINAL_OUTPUT_PAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct AttachQuery {
    #[serde(default = "control")]
    mode: String,
    since: Option<u64>,
    #[serde(default)]
    takeover: bool,
    output_flow: Option<String>,
    bootstrap: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
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
    let mode = requested_mode(state.web_mode, &query.mode);
    let mut args = json!({
        "group_id": group_id,
        "actor_id": actor_id,
        "mode": mode,
        "takeover": mode == "control" && query.takeover,
    });
    if let Some(since) = query.since {
        args["since"] = json!(since);
    }
    if query.bootstrap.as_deref() == Some(SNAPSHOT_V1) {
        args["bootstrap"] = json!(SNAPSHOT_V1);
    }
    if let (Some(cols), Some(rows)) = (query.cols, query.rows) {
        args["cols"] = json!(cols);
        args["rows"] = json!(rows);
    }
    let request = cccc_contracts::DaemonRequest {
        v: 1,
        op: "term_attach".into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    let (response, stream) = match state.client.upgrade(&request).await {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(%group_id, %actor_id, %error, "terminal stream upgrade failed");
            send_terminal_error(
                &mut socket,
                "daemon_unavailable",
                "Terminal service is unavailable.",
            )
            .await;
            return;
        }
    };
    if !response.ok {
        let (code, message) = response.error.map_or_else(
            || {
                (
                    "term_attach_failed".into(),
                    "Terminal attach failed.".into(),
                )
            },
            |error| (error.code, error.message),
        );
        send_terminal_error(&mut socket, &code, &message).await;
        return;
    }
    let mut attach_result = Value::Object(response.result);
    let mut writable = !state.web_mode.is_read_only()
        && attach_result
            .get("terminal_writable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    attach_result["terminal_writable"] = json!(writable);
    let Some(attachment_id) = attach_result.get("attachment_id").and_then(Value::as_u64) else {
        send_terminal_error(
            &mut socket,
            "invalid_daemon_response",
            "Terminal attach response is missing its attachment id.",
        )
        .await;
        return;
    };
    let Some(mut cursor) = attach_result.get("replay_cursor").and_then(Value::as_u64) else {
        send_terminal_error(
            &mut socket,
            "invalid_daemon_response",
            "Terminal attach response is missing its replay cursor.",
        )
        .await;
        return;
    };
    let snapshot = match snapshot_bootstrap(&attach_result) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            send_terminal_error(&mut socket, "invalid_daemon_response", message).await;
            return;
        }
    };
    let mut output_flow = OutputFlow::new(query.output_flow.as_deref());
    if let Some(protocol) = output_flow.protocol() {
        attach_result["output_flow_control"] = json!({
            "protocol": protocol,
            "window_bytes": output_flow.window_bytes(),
        });
    }
    let attach = frame(b'3', attach_result.to_string().as_bytes());
    if socket.send(Message::Binary(attach.into())).await.is_err() {
        return;
    }
    let (mut daemon_read, mut daemon_write) = tokio::io::split(stream);
    if let Some(snapshot) = snapshot {
        let data = match read_snapshot(&mut daemon_read, snapshot).await {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(%group_id, %actor_id, %error, "terminal snapshot read failed");
                send_terminal_error(
                    &mut socket,
                    "daemon_unavailable",
                    "Terminal snapshot connection was interrupted.",
                )
                .await;
                return;
            }
        };
        if !send_output_frame(&mut socket, b'7', &data, snapshot.cursor, &mut output_flow).await {
            return;
        }
    }
    let mut output = [0_u8; TERMINAL_OUTPUT_PAGE_BYTES];
    let mut shutdown = state.shutdown.subscribe();
    let mut writable_poll = tokio::time::interval(Duration::from_millis(100));
    writable_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.recv() => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            _ = writable_poll.tick(), if mode == "control" => {
                let Some(next_writable) = terminal_writable(
                    &state,
                    &group_id,
                    &actor_id,
                    attachment_id,
                ).await else { continue; };
                if next_writable != writable {
                    writable = next_writable;
                    let payload = json!({"terminal_writable": writable});
                    if socket.send(Message::Binary(frame(b'6', payload.to_string().as_bytes()).into())).await.is_err() {
                        break;
                    }
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break; };
                if let Some(cursor) = output_ack_cursor(&message) {
                    output_flow.acknowledge(cursor);
                    continue;
                }
                if !handle_stream_input(
                    &mut socket,
                    TerminalInputContext {
                        state: &state,
                        group_id: &group_id,
                        actor_id: &actor_id,
                        attachment_id,
                        writable,
                    },
                    message,
                    &mut daemon_write,
                )
                .await
                {
                    break;
                }
            }
            read = daemon_read.read(&mut output), if output_flow.can_send(TERMINAL_OUTPUT_PAGE_BYTES) => {
                let count = match read {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) => {
                        tracing::warn!(%group_id, %actor_id, %error, cursor, "terminal stream read failed");
                        send_terminal_error(
                            &mut socket,
                            "daemon_unavailable",
                            "Terminal output connection was interrupted.",
                        )
                        .await;
                        break;
                    }
                };
                cursor = cursor.saturating_add(count as u64);
                if !send_output_frame(&mut socket, b'1', &output[..count], cursor, &mut output_flow).await {
                    break;
                }
            }
        }
    }
}

fn terminal_disabled(web_mode: crate::WebMode, exhibit_allow_terminal: bool) -> bool {
    web_mode.is_read_only() && !exhibit_allow_terminal
}

fn requested_mode(web_mode: crate::WebMode, requested_mode: &str) -> &'static str {
    if web_mode.is_read_only() || requested_mode.trim().eq_ignore_ascii_case("viewer") {
        "viewer"
    } else {
        "control"
    }
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
    use super::{requested_mode, terminal_disabled};
    use crate::WebMode;

    #[test]
    fn exhibit_terminal_is_disabled_by_default_and_never_writable() {
        assert!(terminal_disabled(WebMode::Exhibit, false));
        assert!(!terminal_disabled(WebMode::Exhibit, true));
        assert_eq!(requested_mode(WebMode::Exhibit, "control"), "viewer");
        assert_eq!(requested_mode(WebMode::Normal, "control"), "control");
        assert_eq!(requested_mode(WebMode::Normal, "viewer"), "viewer");
        assert_eq!(requested_mode(WebMode::Normal, " Viewer "), "viewer");
    }
}

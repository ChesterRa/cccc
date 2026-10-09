//! Resident Secretary terminal. Process and task ownership stay in the daemon.
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, State};
use axum::response::Response;
use axum::{Router, routing::get};
use cccc_contracts::DaemonRequest;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::terminal_ws_bootstrap::{SNAPSHOT_V1, read_snapshot, snapshot_bootstrap};
use super::terminal_ws_flow::{OutputFlow, output_ack_cursor};
use super::terminal_ws_protocol::{daemon_call, frame, send_output_frame, terminal_writable};
use crate::{AppState, auth::Principal};

const PAGE_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct AttachQuery {
    generation: String,
    mode: Option<String>,
    #[serde(default)]
    takeover: bool,
    since: Option<u64>,
    bootstrap: Option<String>,
    output_flow: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/voice-secretary/term", get(upgrade))
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<AttachQuery>,
    Extension(principal): Extension<Principal>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| serve(socket, state, query, principal))
}

async fn error(socket: &mut WebSocket, code: &str, message: &str) {
    let _ = socket
        .send(Message::Text(
            json!({"ok":false,"error":{"code":code,"message":message}})
                .to_string()
                .into(),
        ))
        .await;
    let _ = socket.send(Message::Close(None)).await;
}

async fn serve(mut socket: WebSocket, state: AppState, query: AttachQuery, principal: Principal) {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        error(
            &mut socket,
            "permission_denied",
            "The Secretary terminal requires an administrator in interactive Web mode.",
        )
        .await;
        return;
    }
    let mut args = json!({"generation":query.generation,"by":"user", "mode":query.mode, "takeover":query.takeover});
    if let Some(since) = query.since {
        args["since"] = json!(since);
    }
    if query.bootstrap.as_deref() == Some(SNAPSHOT_V1) {
        args["bootstrap"] = json!(SNAPSHOT_V1);
    }
    if let Some(cols) = query.cols {
        args["cols"] = json!(cols);
    }
    if let Some(rows) = query.rows {
        args["rows"] = json!(rows);
    }
    let request = DaemonRequest {
        v: 1,
        op: "voice_secretary_terminal_attach".into(),
        args: args.as_object().expect("terminal args").clone(),
    };
    let (response, mut stream) = match state.client.upgrade(&request).await {
        Ok(value) => value,
        Err(_) => {
            error(
                &mut socket,
                "daemon_unavailable",
                "Terminal service is unavailable.",
            )
            .await;
            return;
        }
    };
    if !response.ok {
        if let Some(failure) = response.error {
            error(&mut socket, &failure.code, &failure.message).await;
        }
        return;
    }
    if !principal.current_admin(&state.home).unwrap_or(false) {
        return;
    }
    let mut result = json!(response.result);
    let Some(attachment_id) = result["attachment_id"].as_u64() else {
        return;
    };
    let mut writable = result["terminal_writable"].as_bool().unwrap_or(false);
    let mut flow = OutputFlow::new(query.output_flow.as_deref());
    if let Some(protocol) = flow.protocol() {
        result["output_flow_control"] =
            json!({"protocol":protocol,"window_bytes":flow.window_bytes()});
    }
    let snapshot = match snapshot_bootstrap(&result) {
        Ok(value) => value,
        Err(reason) => {
            error(&mut socket, "invalid_terminal_snapshot", reason).await;
            return;
        }
    };
    if socket
        .send(Message::Binary(
            frame(b'3', result.to_string().as_bytes()).into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let mut cursor = result["replay_cursor"].as_u64().unwrap_or(0);
    if let Some(snapshot) = snapshot {
        let data = match read_snapshot(&mut stream, snapshot).await {
            Ok(data) => data,
            Err(_) => {
                error(
                    &mut socket,
                    "daemon_unavailable",
                    "Terminal snapshot was interrupted.",
                )
                .await;
                return;
            }
        };
        cursor = snapshot.cursor;
        if !send_output_frame(&mut socket, b'7', &data, cursor, &mut flow).await {
            return;
        }
    }
    let mut shutdown = state.shutdown.subscribe();
    let mut authorization = tokio::time::interval(std::time::Duration::from_secs(1));
    authorization.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut output = vec![0; PAGE_BYTES];
    loop {
        tokio::select! {
            biased;
            _ = shutdown.recv() => break,
            _ = authorization.tick() => {
                if !principal.current_admin(&state.home).unwrap_or(false) { break; }
                if let Some(next) = terminal_writable(&state, "voice-secretary-terminal", &query.generation, attachment_id).await
                    && next != writable {
                    writable = next;
                    if socket.send(Message::Binary(frame(b'6', json!({"terminal_writable":writable}).to_string().as_bytes()).into())).await.is_err() { break; }
                }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break; };
                if !principal.current_admin(&state.home).unwrap_or(false) { break; }
                if let Some(cursor) = output_ack_cursor(&message) { flow.acknowledge(cursor); continue; }
                match terminal_input(&message) {
                    TerminalMessage::Input(payload) if writable => {
                        if stream.write_all(payload).await.is_err() || stream.flush().await.is_err() { break; }
                    }
                    TerminalMessage::Input(_) => {
                        let reply = json!({"type":"terminal.input_ack","ok":false,"error":{"code":"viewer_only","message":"Terminal control moved to another connection. Reconnect to control it."}});
                        if socket.send(Message::Binary(frame(b'4', reply.to_string().as_bytes()).into())).await.is_err() { break; }
                    }
                    TerminalMessage::Resize(cols, rows) if writable => {
                        let resized = daemon_call(&state, "voice_secretary_terminal_resize", json!({"generation":query.generation,"attachment_id":attachment_id,"cols":cols,"rows":rows,"by":"user"})).await;
                        if !resized.is_some_and(|result| result.ok) { break; }
                    }
                    TerminalMessage::Close => break,
                    _ => {},
                }
            }
            result = stream.read(&mut output), if flow.can_send(PAGE_BYTES) => {
                let Ok(bytes) = result else { error(&mut socket, "daemon_unavailable", "Terminal output was interrupted.").await; break; };
                if bytes == 0 { break; }
                cursor = cursor.saturating_add(bytes as u64);
                if !send_output_frame(&mut socket, b'1', &output[..bytes], cursor, &mut flow).await { break; }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

#[derive(Debug, PartialEq)]
enum TerminalMessage<'a> {
    Resize(u16, u16),
    Input(&'a [u8]),
    Close,
    Ignore,
}
fn terminal_input(message: &Message) -> TerminalMessage<'_> {
    let Message::Binary(data) = message else {
        return if matches!(message, Message::Close(_)) {
            TerminalMessage::Close
        } else {
            TerminalMessage::Ignore
        };
    };
    match data.split_first() {
        Some((b'0', payload)) => TerminalMessage::Input(payload),
        Some((b'2', payload)) => {
            let value: Value = serde_json::from_slice(payload).unwrap_or_default();
            match (value["cols"].as_u64(), value["rows"].as_u64()) {
                (Some(cols @ 10..=4096), Some(rows @ 2..=4096)) => {
                    TerminalMessage::Resize(cols as u16, rows as u16)
                }
                _ => TerminalMessage::Ignore,
            }
        }
        _ => TerminalMessage::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_preserves_native_input_and_bounds_resize() {
        assert_eq!(
            terminal_input(&Message::Binary(frame(b'0', b"new task\r").into())),
            TerminalMessage::Input(b"new task\r")
        );
        assert_eq!(
            terminal_input(&Message::Binary(
                frame(b'2', br#"{"cols":80,"rows":20}"#).into()
            )),
            TerminalMessage::Resize(80, 20)
        );
        for payload in [
            br#"{"cols":5000,"rows":20}"#.as_slice(),
            br#"{"cols":80,"rows":0}"#,
            br#"{"cols":80.5,"rows":20}"#,
            b"invalid",
        ] {
            assert_eq!(
                terminal_input(&Message::Binary(frame(b'2', payload).into())),
                TerminalMessage::Ignore
            );
        }
    }
}

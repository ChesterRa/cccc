use super::TerminalQuery;
use axum::extract::ws::{Message, WebSocket};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

use super::super::terminal_ws_flow::{OutputFlow, output_ack_cursor};
use super::super::terminal_ws_protocol::{frame, send_output_frame};
use crate::AppState;

const TERMINAL_OUTPUT_PAGE_BYTES: usize = 64 * 1024;

pub(super) async fn serve(
    mut socket: WebSocket,
    state: AppState,
    session: Arc<crate::codex_voice::AnalystRuntime>,
    query: TerminalQuery,
) {
    let mode = if query.mode.trim().eq_ignore_ascii_case("viewer") {
        cccc_runtime::TerminalAttachMode::Viewer
    } else {
        cccc_runtime::TerminalAttachMode::Control
    };
    let takeover = mode == cccc_runtime::TerminalAttachMode::Control && query.takeover;
    let initial_size = if takeover {
        requested_terminal_size(&query)
    } else {
        None
    };
    let prefer_snapshot = query.bootstrap.as_deref() == Some("snapshot_v1");
    let mut attachment = match session
        .attach_terminal(mode, takeover, query.since, prefer_snapshot, initial_size)
        .await
    {
        Ok(attachment) => attachment,
        Err(error) => {
            tracing::warn!(%error, "Voice Analyst terminal attach failed");
            send_terminal_error(
                &mut socket,
                "codex_voice_terminal_unavailable",
                "Voice Analyst terminal is unavailable.",
            )
            .await;
            return;
        }
    };
    let attachment_id = attachment.attachment_id();
    let mut attachment_writable = attachment.terminal_writable();
    let analyst_input_allowed = session.analyst().terminal_input_allowed().await;
    let mut writable = attachment_writable && analyst_input_allowed;
    let mut attach_result = json!({
        "attachment_id":attachment_id,
        "terminal_mode":attachment.mode().as_str(),
        "terminal_writable":writable,
        "terminal_input_blocked":attachment_writable && !analyst_input_allowed,
        "writer_replaced":attachment.writer_replaced(),
        "terminal_response_owner":"server_v1",
        "replay_cursor":attachment.replay_cursor(),
        "replay_end_cursor":attachment.replay_end_cursor(),
    });
    let initial = attachment.take_initial_output();
    let mut initial_value = json!({
        "kind":initial.kind.as_str(),
        "bytes":initial.data.len(),
        "cursor":initial.end_cursor,
    });
    if let (Some(cols), Some(rows)) = (initial.cols, initial.rows) {
        initial_value["cols"] = json!(cols);
        initial_value["rows"] = json!(rows);
    }
    attach_result["initial_output"] = initial_value;
    let mut output_flow = OutputFlow::new(query.output_flow.as_deref());
    if let Some(protocol) = output_flow.protocol() {
        attach_result["output_flow_control"] = json!({
            "protocol":protocol,
            "window_bytes":output_flow.window_bytes(),
        });
    }
    if socket
        .send(Message::Binary(
            frame(b'3', attach_result.to_string().as_bytes()).into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let initial_opcode = if initial.kind == cccc_runtime::TerminalInitialOutputKind::Snapshot {
        b'7'
    } else {
        b'1'
    };
    if !send_output_frame(
        &mut socket,
        initial_opcode,
        &initial.data,
        initial.end_cursor,
        &mut output_flow,
    )
    .await
    {
        return;
    }

    let input = attachment.input();
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
            _ = writable_poll.tick(), if mode == cccc_runtime::TerminalAttachMode::Control => {
                let Ok(next_attachment_writable) = session.terminal_writable(attachment_id) else { continue; };
                let next_analyst_input_allowed = session.analyst().terminal_input_allowed().await;
                let next_writable = next_attachment_writable && next_analyst_input_allowed;
                attachment_writable = next_attachment_writable;
                if next_writable != writable {
                    writable = next_writable;
                    let payload = json!({"terminal_writable":writable});
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
                if !handle_input(
                    &mut socket,
                    &session,
                    &input,
                    attachment_id,
                    attachment_writable,
                    &mut writable,
                    message,
                ).await {
                    break;
                }
            }
            output = attachment.next_output(TERMINAL_OUTPUT_PAGE_BYTES),
                if output_flow.can_send(TERMINAL_OUTPUT_PAGE_BYTES) => {
                match output {
                    Ok(Some(output)) => {
                        if !send_output_frame(
                            &mut socket, b'1', &output.data, output.end_cursor, &mut output_flow,
                        ).await {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "Voice Analyst terminal output failed");
                        send_terminal_error(
                            &mut socket,
                            "codex_voice_terminal_unavailable",
                            "Voice Analyst terminal output was interrupted.",
                        ).await;
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_input(
    socket: &mut WebSocket,
    session: &crate::codex_voice::AnalystRuntime,
    input: &cccc_runtime::TerminalInput,
    attachment_id: u64,
    attachment_writable: bool,
    writable: &mut bool,
    message: Message,
) -> bool {
    let Message::Binary(data) = message else {
        return !matches!(message, Message::Close(_));
    };
    let Some((&opcode, payload)) = data.split_first() else {
        return true;
    };
    match opcode {
        b'0' if attachment_writable && !payload.is_empty() => {
            if !session.analyst().terminal_input_allowed().await {
                *writable = false;
                let payload = json!({"terminal_writable":false});
                return socket
                    .send(Message::Binary(
                        frame(b'6', payload.to_string().as_bytes()).into(),
                    ))
                    .await
                    .is_ok();
            }
            let input = input.clone();
            let payload = payload.to_vec();
            match tokio::task::spawn_blocking(move || input.write(&payload)).await {
                Ok(Ok(true)) => true,
                Ok(Ok(false)) => {
                    send_input_error(
                        socket,
                        "viewer_only",
                        "Terminal control moved to another connection.",
                    )
                    .await
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Voice Analyst terminal input failed");
                    send_input_error(socket, "write_failed", "Failed to write terminal input.")
                        .await
                }
                Err(error) => {
                    tracing::warn!(%error, "Voice Analyst terminal input task failed");
                    false
                }
            }
        }
        b'0' if attachment_writable => true,
        b'0' => {
            send_input_error(
                socket,
                "viewer_only",
                "This terminal connection is read-only; reconnect as control to write.",
            )
            .await
        }
        b'2' if attachment_writable => {
            let value: Value = serde_json::from_slice(payload).unwrap_or_else(|_| json!({}));
            let Some((cols, rows)) = parsed_terminal_size(&value) else {
                return true;
            };
            session
                .resize_terminal(attachment_id, cols, rows)
                .unwrap_or(false)
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

fn requested_terminal_size(query: &TerminalQuery) -> Option<(u16, u16)> {
    valid_terminal_size(query.cols?, query.rows?)
}

pub(super) fn parsed_terminal_size(value: &Value) -> Option<(u16, u16)> {
    let cols = value
        .get("cols")?
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())?;
    let rows = value
        .get("rows")?
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())?;
    valid_terminal_size(cols, rows)
}

pub(super) fn valid_terminal_size(cols: u16, rows: u16) -> Option<(u16, u16)> {
    ((10..=4096).contains(&cols) && (2..=4096).contains(&rows)).then_some((cols, rows))
}

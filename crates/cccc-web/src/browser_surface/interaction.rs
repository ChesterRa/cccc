use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::future::Future;

use super::BrowserSurfaces;

impl BrowserSurfaces {
    pub async fn command(&self, key: &str, command: &Value) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("browser surface is not active")?;
        match command.get("t").and_then(Value::as_str).unwrap_or("") {
            "ping" => return Ok(()),
            "navigate" => {
                let url = command
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if url.is_empty() {
                    bail!("browser navigate command requires url");
                }
                super::validate_browser_surface_url(url)?;
                super::navigation::goto_dom_content_loaded(&session.page, url).await?;
            }
            "click" => {
                let (button, buttons) = match command
                    .get("button")
                    .and_then(Value::as_str)
                    .unwrap_or("left")
                {
                    "left" => (MouseButton::Left, 1),
                    "middle" => (MouseButton::Middle, 4),
                    "right" => (MouseButton::Right, 2),
                    value => bail!("unsupported browser mouse button: {value}"),
                };
                let existing_pages = session
                    .browser
                    .pages()
                    .await?
                    .into_iter()
                    .map(|page| page.target_id().clone())
                    .collect::<HashSet<_>>();
                let x = number(command, "x");
                let y = number(command, "y");
                if button == MouseButton::Left {
                    let retarget_script = format!(
                        "(() => {{ const node = document.elementFromPoint({x}, {y}); const link = node?.closest?.('a[href]'); if (link?.target === '_blank') link.target = '_self'; }})()"
                    );
                    session.page.evaluate(retarget_script).await?;
                }
                let mut pressed =
                    DispatchMouseEventParams::new(DispatchMouseEventType::MousePressed, x, y);
                pressed.button = Some(button.clone());
                pressed.buttons = Some(buttons);
                pressed.click_count = Some(1);
                session.page.execute(pressed).await?;
                let mut released =
                    DispatchMouseEventParams::new(DispatchMouseEventType::MouseReleased, x, y);
                released.button = Some(button);
                released.buttons = Some(0);
                released.click_count = Some(1);
                session.page.execute(released).await?;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                if let Some(page) = session
                    .browser
                    .pages()
                    .await?
                    .into_iter()
                    .find(|page| !existing_pages.contains(page.target_id()))
                {
                    session.page = page;
                }
            }
            "text" => {
                let text = command.get("text").and_then(Value::as_str).unwrap_or("");
                session.page.execute(InsertTextParams::new(text)).await?;
            }
            "key" => {
                let key = command.get("key").and_then(Value::as_str).unwrap_or("");
                if key.is_empty() {
                    bail!("browser key command requires key");
                }
                press_key(&session.page, key).await?;
            }
            "scroll" => {
                let x = command
                    .get("x")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::from(session.width) / 2.0);
                let y = command
                    .get("y")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::from(session.height) / 2.0);
                let mut wheel =
                    DispatchMouseEventParams::new(DispatchMouseEventType::MouseWheel, x, y);
                wheel.delta_x = Some(number(command, "dx"));
                wheel.delta_y = Some(number(command, "dy"));
                session.page.execute(wheel).await?;
            }
            "back" => {
                super::navigation::go_back_dom_content_loaded(&session.page).await?;
            }
            "refresh" => {
                session.page.reload().await?;
            }
            "resize" => {
                let width = number(command, "width").round().clamp(320.0, 3840.0) as u32;
                let height = number(command, "height").round().clamp(240.0, 2160.0) as u32;
                if !should_override_viewport(
                    session.system_browser.is_some(),
                    (session.width, session.height),
                    (width, height),
                ) {
                    return Ok(());
                }
                session
                    .page
                    .execute(SetDeviceMetricsOverrideParams::new(
                        i64::from(width),
                        i64::from(height),
                        1.0,
                        false,
                    ))
                    .await?;
                session.width = width;
                session.height = height;
                session.updated_at = utc_now();
            }
            _ => bail!("unsupported browser command"),
        }
        session.url = session
            .page
            .url()
            .await?
            .unwrap_or_else(|| session.url.clone());
        session.updated_at = utc_now();
        Ok(())
    }
}

pub async fn serve_socket(
    mut socket: WebSocket,
    surfaces: &BrowserSurfaces,
    key: &str,
    viewer_mode: &str,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let stream_frames = viewer_mode_uses_frames(viewer_mode, surfaces.vnc_port(key).await.is_ok());
    let initial = state_event(surfaces.info(key).await);
    if socket
        .send(Message::Text(initial.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                close_for_shutdown(&mut socket).await;
                break;
            }
            _ = interval.tick(), if stream_frames => match until_shutdown(&mut shutdown, surfaces.frame(key)).await {
                None => {
                    close_for_shutdown(&mut socket).await;
                    break;
                }
                Some(Ok(frame)) => {
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Some(Err(error)) => {
                    let message = error.to_string();
                    let _ = socket.send(Message::Text(
                        json!({"t":"state","active":false,"state":"failed","message":message,"error":{"code":"browser_surface_unavailable","message":message}}).to_string().into(),
                    )).await;
                    break;
                }
            },
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                if matches!(message, Message::Close(_)) { break }
                let Message::Text(text) = message else { continue };
                let Ok(command) = serde_json::from_str::<Value>(&text) else { continue };
                let command_id = command.get("id").and_then(Value::as_str).unwrap_or("");
                let Some(result) = until_shutdown(
                    &mut shutdown,
                    surfaces.command(key, &command),
                ).await else {
                    close_for_shutdown(&mut socket).await;
                    break;
                };
                match result {
                    Ok(()) if !command_id.is_empty() => {
                        let _ = socket.send(Message::Text(
                            json!({"t":"command_result","id":command_id,"ok":true}).to_string().into(),
                        )).await;
                    }
                    Ok(()) => {}
                    Err(error) if !command_id.is_empty() => {
                        let _ = socket.send(Message::Text(
                            json!({"t":"command_result","id":command_id,"ok":false,"message":error.to_string()}).to_string().into(),
                        )).await;
                    }
                    Err(error) => {
                        let _ = socket.send(Message::Text(
                            json!({"t":"error","message":error.to_string()}).to_string().into(),
                        )).await;
                    }
                }
            }
        }
    }
}

fn state_event(mut state: Value) -> Value {
    if let Some(object) = state.as_object_mut() {
        object.insert("t".into(), json!("state"));
    }
    state
}

fn viewer_mode_uses_frames(mode: &str, vnc_available: bool) -> bool {
    mode.trim().eq_ignore_ascii_case("screencast") || !vnc_available
}

pub async fn serve_vnc_socket(
    mut socket: WebSocket,
    surfaces: &BrowserSurfaces,
    key: &str,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(port) = surfaces.vnc_port(key).await else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let Ok(stream) = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await
    else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };
    let (mut vnc_reader, mut vnc_writer) = stream.into_split();
    let (mut sender, mut receiver) = socket.split();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                let _ = sender.send(Message::Close(None)).await;
                break;
            }
            read = vnc_reader.read(&mut buffer) => match read {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if sender
                        .send(Message::Binary(buffer[..read].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            },
            message = receiver.next() => {
                let Some(Ok(message)) = message else { break };
                match message {
                    Message::Binary(bytes) => {
                        if vnc_writer.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn until_shutdown<T>(
    shutdown: &mut tokio::sync::broadcast::Receiver<()>,
    operation: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        _ = shutdown.recv() => None,
        result = operation => Some(result),
    }
}

async fn close_for_shutdown(socket: &mut WebSocket) {
    let _ = socket.send(Message::Close(None)).await;
}

fn should_override_viewport(
    system_browser: bool,
    current: (u32, u32),
    requested: (u32, u32),
) -> bool {
    !system_browser && current != requested
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

async fn press_key(page: &Page, key: &str) -> Result<()> {
    let definition = chromiumoxide::keys::get_key_definition(key)
        .with_context(|| format!("unsupported browser key: {key}"))?;
    let mut command = DispatchKeyEventParams::builder()
        .key(definition.key)
        .code(definition.code)
        .windows_virtual_key_code(definition.key_code)
        .native_virtual_key_code(definition.key_code);
    let down_type = if let Some(text) = definition.text {
        command = command.text(text);
        DispatchKeyEventType::KeyDown
    } else if definition.key.len() == 1 {
        command = command.text(definition.key);
        DispatchKeyEventType::KeyDown
    } else {
        DispatchKeyEventType::RawKeyDown
    };
    page.execute(
        command
            .clone()
            .r#type(down_type)
            .build()
            .map_err(anyhow::Error::msg)?,
    )
    .await?;
    page.execute(
        command
            .r#type(DispatchKeyEventType::KeyUp)
            .build()
            .map_err(anyhow::Error::msg)?,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{should_override_viewport, state_event, until_shutdown, viewer_mode_uses_frames};
    use serde_json::json;
    use std::future::pending;

    #[test]
    fn visible_system_browser_keeps_its_native_viewport_stable() {
        assert!(!should_override_viewport(true, (1366, 900), (900, 640)));
    }

    #[test]
    fn headless_browser_only_resizes_when_dimensions_change() {
        assert!(!should_override_viewport(false, (800, 600), (800, 600)));
        assert!(should_override_viewport(false, (800, 600), (1024, 768)));
    }

    #[test]
    fn initial_socket_state_preserves_vnc_capability() {
        let state = state_event(json!({
            "active":true,
            "state":"ready",
            "viewer":{"kind":"vnc","vnc":{"available":true}}
        }));

        assert_eq!(state["t"], "state");
        assert_eq!(state["viewer"]["kind"], "vnc");
        assert_eq!(state["viewer"]["vnc"]["available"], true);
    }

    #[test]
    fn vnc_viewers_do_not_duplicate_cdp_frame_capture() {
        assert!(!viewer_mode_uses_frames("auto", true));
        assert!(viewer_mode_uses_frames("screencast", true));
        assert!(viewer_mode_uses_frames("auto", false));
    }

    #[tokio::test]
    async fn shutdown_cancels_a_stalled_browser_operation() {
        let (shutdown, mut receiver) = tokio::sync::broadcast::channel(1);
        shutdown.send(()).expect("shutdown receiver");

        assert!(
            until_shutdown(&mut receiver, pending::<()>())
                .await
                .is_none()
        );
    }
}

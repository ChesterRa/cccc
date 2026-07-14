use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::layout::Point;
use chromiumoxide::page::ScreenshotParams;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Default)]
pub struct BrowserSurfaces {
    sessions: Mutex<HashMap<String, Session>>,
}

struct Session {
    browser: Browser,
    page: Page,
    handler: JoinHandle<()>,
    url: String,
    width: u32,
    height: u32,
    started_at: String,
    updated_at: String,
    seq: u64,
}

impl BrowserSurfaces {
    pub async fn open(
        &self,
        key: &str,
        profile: &Path,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<Value> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("browser surface URL must use http or https");
        }
        self.close(key).await?;
        std::fs::create_dir_all(profile)?;
        let config = BrowserConfig::builder()
            .new_headless_mode()
            .window_size(width, height)
            .user_data_dir(profile)
            .build()
            .map_err(anyhow::Error::msg)?;
        let (browser, mut handler) = Browser::launch(config).await?;
        let task = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if message.is_err() {
                    break;
                }
            }
        });
        let page = browser.new_page(url).await.context("open browser page")?;
        let now = utc_now();
        let session = Session {
            browser,
            page,
            handler: task,
            url: url.into(),
            width,
            height,
            started_at: now.clone(),
            updated_at: now,
            seq: 0,
        };
        let state = state(&session);
        self.sessions.lock().await.insert(key.into(), session);
        Ok(state)
    }

    pub async fn info(&self, key: &str) -> Value {
        self.sessions
            .lock()
            .await
            .get(key)
            .map(state)
            .unwrap_or_else(idle)
    }

    pub async fn close(&self, key: &str) -> Result<bool> {
        let session = self.sessions.lock().await.remove(key);
        let Some(mut session) = session else {
            return Ok(false);
        };
        let result = session.browser.close().await;
        session.handler.abort();
        result?;
        Ok(true)
    }

    pub async fn frame(&self, key: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("browser surface is not active")?;
        let bytes = session
            .page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Jpeg)
                    .quality(75)
                    .build(),
            )
            .await?;
        session.seq += 1;
        session.updated_at = utc_now();
        session.url = session
            .page
            .url()
            .await?
            .unwrap_or_else(|| session.url.clone());
        Ok(json!({
            "t":"frame",
            "seq":session.seq,
            "mime":"image/jpeg",
            "data_base64":base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
            "width":session.width,
            "height":session.height,
            "captured_at":session.updated_at,
            "url":session.url
        }))
    }

    pub async fn command(&self, key: &str, command: &Value) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("browser surface is not active")?;
        match command.get("t").and_then(Value::as_str).unwrap_or("") {
            "click" => {
                session
                    .page
                    .click(Point::new(number(command, "x"), number(command, "y")))
                    .await?;
            }
            "text" => {
                let text = command.get("text").and_then(Value::as_str).unwrap_or("");
                session.page.execute(InsertTextParams::new(text)).await?;
            }
            "key" => {
                let key = command.get("key").and_then(Value::as_str).unwrap_or("");
                let script = format!(
                    "document.activeElement?.dispatchEvent(new KeyboardEvent('keydown',{{key:{}}}))",
                    serde_json::to_string(key)?
                );
                session.page.evaluate(script).await?;
            }
            "scroll" => {
                let script = format!(
                    "window.scrollBy({}, {})",
                    number(command, "dx"),
                    number(command, "dy")
                );
                session.page.evaluate(script).await?;
            }
            "back" => {
                session.page.evaluate("history.back()").await?;
            }
            "refresh" => {
                session.page.reload().await?;
            }
            "resize" => {
                let width = number(command, "width").round().clamp(320.0, 3840.0) as u32;
                let height = number(command, "height").round().clamp(240.0, 2160.0) as u32;
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
        Ok(())
    }
}

pub async fn serve_socket(mut socket: WebSocket, surfaces: &BrowserSurfaces, key: &str) {
    if socket
        .send(Message::Text(
            json!({"t":"state","active":true,"state":"ready"})
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(300));
    loop {
        tokio::select! {
            _ = interval.tick() => match surfaces.frame(key).await {
                Ok(frame) => {
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = socket.send(Message::Text(
                        json!({"t":"error","message":error.to_string()}).to_string().into(),
                    )).await;
                    break;
                }
            },
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break };
                if matches!(message, Message::Close(_)) { break }
                let Message::Text(text) = message else { continue };
                let Ok(command) = serde_json::from_str::<Value>(&text) else { continue };
                if let Err(error) = surfaces.command(key, &command).await {
                    let _ = socket.send(Message::Text(
                        json!({"t":"error","message":error.to_string()}).to_string().into(),
                    )).await;
                }
            }
        }
    }
}

fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn state(session: &Session) -> Value {
    json!({
        "active":true,"state":"ready","message":"Browser surface is ready.","strategy":"cdp_screencast",
        "url":session.url,"width":session.width,"height":session.height,
        "started_at":session.started_at,"updated_at":session.updated_at,
        "last_frame_seq":session.seq,"last_frame_at":session.updated_at,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"VNC is not configured"}}
    })
}

fn idle() -> Value {
    json!({
        "active":false,"state":"idle","message":"No browser surface session is active for this slot.",
        "width":0,"height":0,"last_frame_seq":0,"controller_attached":false,
        "viewer":{"kind":"screencast","vnc":{"available":false,"error":"VNC is not configured"}}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn launches_chromium_and_captures_nonempty_frame() {
        if !chrome_available() {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            let body = "<!doctype html><html><body style='background:#fff'><h1>CCCC browser frame</h1><input autofocus></body></html>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .expect("response");
        });
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = BrowserSurfaces::default();
        let url = format!("http://{address}");
        let state = manager
            .open(
                "g_test::slot-1",
                &temp.path().join("profile"),
                &url,
                800,
                600,
            )
            .await
            .expect("open");
        assert_eq!(state["state"], "ready");
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let frame = manager.frame("g_test::slot-1").await.expect("frame");
        let image = base64::engine::general_purpose::STANDARD
            .decode(frame["data_base64"].as_str().expect("base64"))
            .expect("jpeg");
        assert!(image.len() > 1_000);
        assert_eq!(&image[..2], &[0xff, 0xd8]);
        assert!(manager.close("g_test::slot-1").await.expect("close"));
        server.await.expect("server");
    }

    fn chrome_available() -> bool {
        [
            "/opt/homebrew/bin/chromium",
            "/usr/bin/chromium",
            "/usr/bin/google-chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ]
        .iter()
        .any(|path| std::path::Path::new(path).is_file())
    }
}

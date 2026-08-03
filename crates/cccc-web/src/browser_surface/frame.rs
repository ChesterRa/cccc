use anyhow::{Context, Result};
use base64::Engine;
use cccc_contracts::utc_now;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use serde::Deserialize;
use serde_json::{Value, json};

use super::BrowserSurfaces;
use super::page_recovery::{is_page_gone, recover_page};

#[derive(Deserialize)]
struct ViewportSize {
    width: u32,
    height: u32,
}

impl BrowserSurfaces {
    pub async fn frame(&self, key: &str) -> Result<Value> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(key)
            .context("browser surface is not active")?;
        let (bytes, viewport) = match capture_frame(&session.page).await {
            Ok(frame) => frame,
            Err(error) if session.recover_closed_page && is_page_gone(&error) => {
                tracing::warn!(%error, "browser tab closed; recreating projected surface page");
                recover_page(session).await?;
                capture_frame(&session.page).await?
            }
            Err(error) => return Err(error),
        };
        session.width = viewport.width;
        session.height = viewport.height;
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
            "data_base64":base64::engine::general_purpose::STANDARD.encode(bytes),
            "width":session.width,
            "height":session.height,
            "captured_at":session.updated_at,
            "url":session.url
        }))
    }
}

async fn capture_frame(page: &Page) -> Result<(Vec<u8>, ViewportSize)> {
    let viewport = page
        .evaluate("({ width: window.innerWidth, height: window.innerHeight })")
        .await
        .context("read projected browser viewport")?
        .into_value::<ViewportSize>()
        .context("decode projected browser viewport")?;
    let bytes = page
        .screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Jpeg)
                .quality(75)
                .build(),
        )
        .await?;
    Ok((bytes, viewport))
}

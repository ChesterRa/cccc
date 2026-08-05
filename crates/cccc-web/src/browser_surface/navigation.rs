use anyhow::{Context, Result, bail};
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::page::EventDomContentEventFired;
use futures_util::StreamExt;
use serde::Deserialize;

const NAVIGATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Deserialize)]
struct DocumentState {
    document_uri: String,
    ready_state: String,
}

pub(super) async fn goto_dom_content_loaded(page: &Page, url: &str) -> Result<()> {
    let mut events = page
        .event_listener::<EventDomContentEventFired>()
        .await
        .context("listen for DOMContentLoaded")?;
    let encoded_url = serde_json::to_string(url)?;
    page.evaluate(format!("window.location.assign({encoded_url})"))
        .await
        .context("start browser navigation")?;
    tokio::time::timeout(NAVIGATION_TIMEOUT, events.next())
        .await
        .context("browser navigation timed out waiting for DOMContentLoaded")?
        .context("browser closed before DOMContentLoaded")?;

    let state = page
        .evaluate(
            "({ document_uri: document.documentURI || '', ready_state: document.readyState || '' })",
        )
        .await
        .context("inspect loaded browser document")?
        .into_value::<DocumentState>()
        .context("decode loaded browser document state")?;
    if state.document_uri.starts_with("chrome-error://") {
        bail!("Chromium loaded an internal network error page");
    }
    if !matches!(state.ready_state.as_str(), "interactive" | "complete") {
        bail!("browser document did not reach DOMContentLoaded");
    }
    Ok(())
}

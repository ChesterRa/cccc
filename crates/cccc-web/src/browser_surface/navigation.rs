use anyhow::{Context, Result, bail};
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::page::{
    EventDomContentEventFired, GetNavigationHistoryParams, NavigateToHistoryEntryParams,
};
use futures_util::StreamExt;
use serde::Deserialize;

const NAVIGATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const BACK_NAVIGATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const PREVIOUS_DOCUMENT_MARKER: &str = "__cccc_browser_surface_navigation_pending_v1";

#[derive(Deserialize)]
struct DocumentState {
    document_uri: String,
    ready_state: String,
    previous_document: bool,
}

pub(super) async fn goto_dom_content_loaded(page: &Page, url: &str) -> Result<()> {
    let mut events = page
        .event_listener::<EventDomContentEventFired>()
        .await
        .context("listen for DOMContentLoaded")?;
    let encoded_url = serde_json::to_string(url)?;
    let encoded_marker = serde_json::to_string(PREVIOUS_DOCUMENT_MARKER)?;
    page.evaluate(format!(
        "globalThis[{encoded_marker}] = true; window.location.assign({encoded_url})"
    ))
    .await
    .context("start browser navigation")?;
    tokio::time::timeout(NAVIGATION_TIMEOUT, async {
        loop {
            events
                .next()
                .await
                .context("browser closed before DOMContentLoaded")?;
            let state = document_state(page).await?;
            if state.previous_document {
                continue;
            }
            if state.document_uri.starts_with("chrome-error://") {
                bail!("Chromium loaded an internal network error page");
            }
            if matches!(state.ready_state.as_str(), "interactive" | "complete") {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await
    .context("browser navigation timed out waiting for DOMContentLoaded")??;

    Ok(())
}

pub(super) async fn go_back_dom_content_loaded(page: &Page) -> Result<()> {
    let history = page
        .execute(GetNavigationHistoryParams::default())
        .await
        .context("read browser navigation history")?;
    let Some(previous_index) = history
        .current_index
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
    else {
        return Ok(());
    };
    let Some(previous) = history.entries.get(previous_index) else {
        return Ok(());
    };
    let target_url = previous.url.clone();
    page.execute(NavigateToHistoryEntryParams::new(previous.id))
        .await
        .context("start browser back navigation")?;

    tokio::time::timeout(BACK_NAVIGATION_TIMEOUT, async {
        loop {
            let observed_url = page.url().await?.unwrap_or_default();
            if observed_url == target_url {
                let ready_state = page
                    .evaluate("document.readyState")
                    .await?
                    .into_value::<String>()?;
                if matches!(ready_state.as_str(), "interactive" | "complete") {
                    return Ok::<(), anyhow::Error>(());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .context("browser back navigation timed out")??;
    Ok(())
}

async fn document_state(page: &Page) -> Result<DocumentState> {
    let marker = serde_json::to_string(PREVIOUS_DOCUMENT_MARKER)?;
    page.evaluate(format!(
        "({{ document_uri: document.documentURI || '', ready_state: document.readyState || '', previous_document: globalThis[{marker}] === true }})"
    ))
    .await
    .context("inspect loaded browser document")?
    .into_value::<DocumentState>()
    .context("decode loaded browser document state")
}

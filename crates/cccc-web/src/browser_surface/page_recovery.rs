use anyhow::{Context, Result};
use chromiumoxide::{Browser, Page};

use super::Session;

pub(super) fn is_page_gone(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let message = cause.to_string().to_ascii_lowercase();
        message.contains("receiver is gone")
            || message.contains("target closed")
            || message.contains("session closed")
            || message.contains("session with given id not found")
            || message.contains("no such target")
    })
}

pub(super) async fn recover_page(session: &mut Session) -> Result<()> {
    let mut stale_pages = Vec::new();
    let mut live_page = None;
    for page in session.browser.pages().await? {
        let url = page.url().await?.unwrap_or_default();
        if is_internal_page(&url) {
            stale_pages.push(page);
        } else {
            live_page = Some(page);
            break;
        }
    }
    let page = match live_page {
        Some(page) => page,
        None => session
            .browser
            .new_page(&session.url)
            .await
            .context("recreate closed browser tab")?,
    };
    for stale_page in stale_pages {
        let _ = stale_page.close().await;
    }
    session.url = page.url().await?.unwrap_or_else(|| session.url.clone());
    session.page = page;
    session.updated_at = cccc_contracts::utc_now();
    Ok(())
}

pub(super) async fn close_internal_pages(browser: &Browser, active: &Page) -> Result<()> {
    let active_id = active.target_id();
    for page in browser.pages().await? {
        if page.target_id() == active_id {
            continue;
        }
        let url = page.url().await?.unwrap_or_default();
        if is_internal_page(&url) {
            let _ = page.close().await;
        }
    }
    Ok(())
}

pub(super) fn is_internal_page(url: &str) -> bool {
    url.is_empty()
        || url == "about:blank"
        || url.starts_with("chrome://new-tab-page")
        || url.starts_with("chrome://newtab")
}

#[cfg(test)]
mod tests {
    use super::is_page_gone;

    #[test]
    fn classifies_only_closed_cdp_page_errors_as_recoverable() {
        assert!(is_page_gone(&anyhow::anyhow!(
            "send failed because receiver is gone"
        )));
        assert!(is_page_gone(&anyhow::anyhow!("Target closed")));
        assert!(is_page_gone(&anyhow::anyhow!(
            "Session with given id not found"
        )));
        assert!(!is_page_gone(&anyhow::anyhow!(
            "Chromium loaded an internal network error page"
        )));
    }
}

use super::super::page_recovery::{candidate_page_url, confirm_candidate_gone};
use super::*;
use chromiumoxide::cdp::browser_protocol::target::CloseTargetParams;
use futures_util::StreamExt;
use std::time::Duration;

// Get a real chromiumoxide canceled-oneshot error without provider traffic or
// killing the browser: drop an extra CDP handler while its URL request is pending.
async fn canceled_url_request(browser: &Browser, page: &Page) -> anyhow::Error {
    let (connection, mut handler) = Browser::connect(browser.websocket_address().clone())
        .await
        .expect("extra CDP connection");
    let (pause_tx, mut pause_rx) = tokio::sync::oneshot::channel();
    let (paused_tx, paused_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut pause_rx => { let _ = paused_tx.send(()); std::future::pending::<()>().await; break; }
                event = handler.next() => { if !matches!(event, Some(Ok(_))) { break; } }
            }
        }
    });
    let copied = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match connection.get_page(page.target_id().clone()).await {
                Ok(page) => break page,
                Err(chromiumoxide::error::CdpError::NotFound) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("extra connection discovery failed: {error}"),
            }
        }
    })
    .await
    .expect("extra connection discovers the existing page");
    copied.url().await.expect("initialized page");
    pause_tx.send(()).expect("fixture operation");
    paused_rx.await.expect("fixture operation");
    let mut pending = Box::pin(copied.url());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), pending.as_mut())
            .await
            .is_err()
    );
    task.abort();
    let _ = task.await;
    pending.await.expect_err("canceled request").into()
}

#[tokio::test]
async fn candidate_enumeration_skips_only_confirmed_disappeared_targets() {
    require_chrome!();
    let temp = tempfile::tempdir().expect("tempdir");
    let (url, server) = local_page("Keep this active page").await;
    let url = reqwest::Url::parse(&url)
        .expect("fixture operation")
        .to_string();
    let manager = BrowserSurfaces::default();
    manager
        .open(
            "candidate-pages",
            &temp.path().join("profile"),
            &url,
            800,
            600,
        )
        .await
        .expect("open");
    {
        let mut sessions = manager.sessions.lock().await;
        let session = sessions
            .get_mut("candidate-pages")
            .expect("fixture operation");
        let candidate = session
            .browser
            .new_page("about:blank")
            .await
            .expect("candidate");
        let canceled = canceled_url_request(&session.browser, &candidate).await;
        assert!(format!("{canceled:#}").contains("oneshot canceled"));
        let error = confirm_candidate_gone(&session.browser, &candidate, canceled)
            .await
            .expect_err("live target error must not be hidden");
        session
            .browser
            .execute(CloseTargetParams::new(candidate.target_id().clone()))
            .await
            .expect("close target");
        confirm_candidate_gone(&session.browser, &candidate, error)
            .await
            .expect("disappeared target is skippable");
        assert!(
            candidate_page_url(&session.browser, &candidate)
                .await
                .expect("stale page")
                .is_none()
        );
        assert_eq!(
            candidate_page_url(&session.browser, &session.page)
                .await
                .expect("fixture operation"),
            Some(url.clone())
        );
        assert!(
            confirm_candidate_gone(
                &session.browser,
                &candidate,
                anyhow::anyhow!("unrelated navigation error")
            )
            .await
            .is_err()
        );
        assert!(
            reusable_page(&session.browser)
                .await
                .expect("enumeration")
                .is_none()
        );
        close_internal_pages(&session.browser, &session.page)
            .await
            .expect("cleanup");
        assert_eq!(
            session.page.url().await.expect("fixture operation"),
            Some(url)
        );
        session.handler.abort();
        assert!(
            candidate_page_url(&session.browser, &session.page)
                .await
                .is_err(),
            "whole connection loss must remain an error"
        );
    }
    manager.shutdown_all().await.expect("shutdown fixture");
    server.abort();
}

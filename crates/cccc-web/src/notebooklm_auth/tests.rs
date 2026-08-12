use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::watch;

use super::*;

fn active(session_id: &str) -> ActiveFlow {
    let (cancel, _) = watch::channel(false);
    ActiveFlow {
        session_id: session_id.into(),
        profile: PathBuf::from("unused"),
        cancel,
        task: None,
    }
}

#[test]
fn clamps_auth_timeout() {
    assert_eq!(run::auth_timeout(1), Duration::from_secs(60));
    assert_eq!(run::auth_timeout(900), Duration::from_secs(900));
    assert_eq!(run::auth_timeout(9_000), Duration::from_secs(1_800));
}

#[tokio::test]
async fn stale_session_cannot_update_current_flow() {
    let manager = AuthFlowManager::default();
    manager.inner.lock().await.active = Some(active("current"));
    manager
        .update("stale", "failed", "failed", "stale update", None)
        .await;
    let inner = manager.inner.lock().await;
    assert_eq!(inner.state.state, "idle");
    assert_eq!(
        inner.active.as_ref().expect("active flow").session_id,
        "current"
    );
}

#[tokio::test]
async fn finish_only_removes_matching_session() {
    let manager = AuthFlowManager::default();
    manager.inner.lock().await.active = Some(active("current"));
    manager
        .finish("stale", "succeeded", "done", "stale finish", None)
        .await;
    assert_eq!(
        manager
            .inner
            .lock()
            .await
            .active
            .as_ref()
            .expect("active flow")
            .session_id,
        "current"
    );
}

#[tokio::test]
async fn finish_retains_matching_session_until_cleanup_completes() {
    let manager = AuthFlowManager::default();
    manager.inner.lock().await.active = Some(active("current"));

    manager
        .finish("current", "succeeded", "done", "connected", None)
        .await;

    {
        let inner = manager.inner.lock().await;
        assert_eq!(inner.state.state, "succeeded");
        assert!(!inner.state.finished_at.is_empty());
        assert_eq!(
            inner
                .active
                .as_ref()
                .expect("cleanup owns the flow")
                .session_id,
            "current"
        );
    }

    manager.clear_active("current").await;
    assert!(manager.inner.lock().await.active.is_none());
}

#[tokio::test]
async fn shutdown_does_not_replace_idle_state() {
    let manager = AuthFlowManager::default();
    let browsers = BrowserSurfaces::default();
    manager.shutdown(&browsers).await;
    let inner = manager.inner.lock().await;
    assert_eq!(inner.state.state, "idle");
    assert!(inner.active.is_none());
}

#[tokio::test]
async fn cancel_removes_active_flow_and_preserves_session_identity() {
    let manager = AuthFlowManager::default();
    {
        let mut inner = manager.inner.lock().await;
        inner.state.session_id = "current".into();
        inner.active = Some(active("current"));
    }
    manager
        .cancel(&BrowserSurfaces::default(), "Canceled by test.")
        .await;
    let inner = manager.inner.lock().await;
    assert_eq!(inner.state.state, "canceled");
    assert_eq!(inner.state.session_id, "current");
    assert!(inner.active.is_none());
}

use crate::browser_surface::BrowserSurfaces;
use std::time::Duration;

// Allow the owner's bounded close/wait/kill sequence to release the shared
// profile and auxiliaries before Web exits. The outer bound still handles stalls.
const BROWSER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(12);
// Long enough for a claimed message to show in the conversation and be
// recorded (its evidence wait is 8 s). Work still running after this is
// recovered from its record by the next Web, never submitted again.
const BROWSER_WORK_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn browser_surfaces(surfaces: &BrowserSurfaces) {
    // Nothing is claimed from here on; work already claimed reaches its record
    // before the browser it runs in is let go.
    surfaces.stop_opening();
    if !crate::routes::wait_for_browser_work(BROWSER_WORK_DRAIN_TIMEOUT).await {
        tracing::warn!(
            "browser work was still running when the Web stopped; the next start recovers it from its record"
        );
    }
    match browser_surfaces_with_timeout(surfaces, BROWSER_SHUTDOWN_TIMEOUT).await {
        Ok(Ok(closed)) if closed > 0 => {
            tracing::info!(closed, "closed browser surfaces during Web shutdown");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to close browser surfaces during Web shutdown");
        }
        Err(_) => {
            tracing::warn!("browser surface shutdown timed out; forcing Web shutdown");
        }
    }
}

async fn browser_surfaces_with_timeout(
    surfaces: &BrowserSurfaces,
    timeout: Duration,
) -> Result<anyhow::Result<usize>, tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, surfaces.shutdown_all()).await
}

#[cfg(test)]
mod tests {
    use super::{BrowserSurfaces, browser_surfaces_with_timeout};
    use std::time::Duration;

    #[tokio::test]
    async fn empty_owner_registry_does_not_wait_for_a_page_registry_lock() {
        let surfaces = BrowserSurfaces::default();
        let _sessions = surfaces.sessions.lock().await;

        let result = browser_surfaces_with_timeout(&surfaces, Duration::from_millis(10)).await;

        assert_eq!(result.expect("no process resources").expect("shutdown"), 0);
    }
}

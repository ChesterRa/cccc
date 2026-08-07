use crate::browser_surface::BrowserSurfaces;
use std::time::Duration;

const BROWSER_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) async fn browser_surfaces(surfaces: &BrowserSurfaces) {
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
    async fn stalled_browser_lock_cannot_block_web_shutdown() {
        let surfaces = BrowserSurfaces::default();
        let _sessions = surfaces.sessions.lock().await;

        let result = browser_surfaces_with_timeout(&surfaces, Duration::from_millis(10)).await;

        assert!(result.is_err(), "stalled browser shutdown must time out");
    }
}

mod api;
mod auth;
mod browser_surface;
mod im_runtime;
mod ledger_event_hub;
mod routes;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use rust_embed::RustEmbed;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

const GRACEFUL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
const COMPONENT_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebAssets;

#[derive(Clone)]
pub(crate) struct AppState {
    client: DaemonClient,
    home: HomeLayout,
    browser_surfaces: Arc<browser_surface::BrowserSurfaces>,
    ledger_events: ledger_event_hub::LedgerEventHub,
    im_workers: Arc<im_runtime::ImWorkerRegistry>,
    shutdown: broadcast::Sender<()>,
}

pub fn app(home: HomeLayout) -> Router {
    let (shutdown, _) = broadcast::channel(1);
    app_with_shutdown(home, shutdown).0
}

fn app_with_shutdown(
    home: HomeLayout,
    shutdown: broadcast::Sender<()>,
) -> (Router, Arc<im_runtime::ImWorkerRegistry>) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    let im_workers = Arc::new(im_runtime::ImWorkerRegistry::new(ledger_events.clone()));
    im_workers.restore_enabled(home.clone(), DaemonClient::new(home.clone()));
    let state = AppState {
        client: DaemonClient::new(home.clone()),
        home,
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
        ledger_events,
        im_workers: Arc::clone(&im_workers),
        shutdown,
    };
    let app = routes::router()
        .fallback(static_asset)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ))
        .with_state(state);
    (app, im_workers)
}

async fn static_asset(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let path = requested.strip_prefix("ui/").unwrap_or(requested);
    let path = if path.is_empty() || path == "ui" {
        "index.html"
    } else {
        path
    };
    let asset = WebAssets::get(path).or_else(|| {
        (!path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.')))
        .then(|| WebAssets::get("index.html"))
        .flatten()
    });
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    (
        [
            (header::CONTENT_TYPE, mime.as_ref()),
            (
                header::CACHE_CONTROL,
                if path.starts_with("assets/") {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                },
            ),
        ],
        Body::from(asset.data.into_owned()),
    )
        .into_response()
}

pub async fn serve(home: HomeLayout, host: &str, port: u16) -> Result<SocketAddr> {
    serve_until(home, host, port, std::future::pending()).await
}

pub async fn serve_until<F>(
    home: HomeLayout,
    host: &str,
    port: u16,
    shutdown: F,
) -> Result<SocketAddr>
where
    F: Future<Output = ()> + Send + 'static,
{
    home.initialize()?;
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let address = listener.local_addr()?;
    ensure_listener_auth(&home, address)?;
    println!("CCCC Web listening on http://{address}");
    tracing::info!(%address, "CCCC Rust Web listening");
    let (web_shutdown, _) = broadcast::channel(1);
    let (shutdown_started, mut shutdown_started_rx) = tokio::sync::oneshot::channel();
    let (app, im_workers) = app_with_shutdown(home, web_shutdown.clone());
    let server = async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = shutdown => {},
                    _ = shutdown_signal() => {},
                }
                let _ = web_shutdown.send(());
                let _ = shutdown_started.send(());
            })
            .await
    };
    tokio::pin!(server);
    let server_result = tokio::select! {
        biased;
        result = &mut server => result,
        _ = &mut shutdown_started_rx => {
            match tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, &mut server).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!("Web graceful shutdown timed out; closing active connections");
                    Ok(())
                }
            }
        }
    };
    if tokio::time::timeout(COMPONENT_SHUTDOWN_TIMEOUT, im_workers.shutdown())
        .await
        .is_err()
    {
        tracing::warn!("Web component shutdown timed out; cancelling remaining IM workers");
    }
    server_result?;
    Ok(address)
}

fn ensure_listener_auth(home: &HomeLayout, address: SocketAddr) -> Result<()> {
    let explicitly_allowed = std::env::var("CCCC_WEB_ALLOW_UNAUTHENTICATED")
        .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "yes"));
    if !address.ip().is_loopback()
        && !explicitly_allowed
        && !AccessTokenStore::new(home.clone())?
            .list()?
            .iter()
            .any(|token| token.is_admin)
    {
        anyhow::bail!(
            "refusing non-loopback Web listener without an administrator access token; use CCCC_WEB_ALLOW_UNAUTHENTICATED=1 only behind a trusted local network boundary"
        );
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let terminate = signal(SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use futures_util::StreamExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn explicit_shutdown_stops_web_server() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            serve_until(home, "127.0.0.1", 0, async {}),
        )
        .await
        .expect("Web shutdown timeout")
        .expect("Web result");
        assert!(result.port() > 0);
    }

    #[test]
    fn remote_listener_requires_an_access_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_err());
        assert!(ensure_listener_auth(&home, "127.0.0.1:8848".parse().expect("address")).is_ok());
    }
    #[tokio::test]
    async fn shutdown_closes_active_sse_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let (shutdown, _) = broadcast::channel(1);
        let response = app_with_shutdown(home, shutdown.clone())
            .0
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/v1/events/stream")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("SSE response");
        let mut body = response.into_body().into_data_stream();
        tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
            .await
            .expect("connected event timeout")
            .expect("connected event missing")
            .expect("connected event");
        shutdown.send(()).expect("active SSE subscriber");
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
                .await
                .expect("SSE shutdown timeout")
                .is_none()
        );
    }
}

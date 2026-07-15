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
use rust_embed::RustEmbed;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

const GRACEFUL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
}

pub fn app(home: HomeLayout) -> Router {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    let im_workers = Arc::new(im_runtime::ImWorkerRegistry::default());
    im_workers.restore_enabled(home.clone(), DaemonClient::new(home.clone()));
    let state = AppState {
        client: DaemonClient::new(home.clone()),
        home,
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
        ledger_events,
        im_workers,
    };
    routes::router()
        .fallback(static_asset)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ))
        .with_state(state)
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
    println!("CCCC Web listening on http://{address}");
    tracing::info!(%address, "CCCC Rust Web listening");
    let (shutdown_started, mut shutdown_started_rx) = tokio::sync::oneshot::channel();
    let server = async move {
        axum::serve(listener, app(home))
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = shutdown => {},
                    _ = shutdown_signal() => {},
                }
                let _ = shutdown_started.send(());
            })
            .await
    };
    tokio::pin!(server);
    tokio::select! {
        biased;
        result = &mut server => result?,
        _ = &mut shutdown_started_rx => {
            match tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, &mut server).await {
                Ok(result) => result?,
                Err(_) => {
                    tracing::warn!("Web graceful shutdown timed out; closing active connections");
                }
            }
        }
    }
    Ok(address)
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
}

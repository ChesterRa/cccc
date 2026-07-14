mod api;
mod auth;
mod browser_surface;
mod routes;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebAssets;

#[derive(Clone)]
pub(crate) struct AppState {
    client: DaemonClient,
    home: HomeLayout,
    browser_surfaces: Arc<browser_surface::BrowserSurfaces>,
}

pub fn app(home: HomeLayout) -> Router {
    let state = AppState {
        client: DaemonClient::new(home.clone()),
        home,
        browser_surfaces: Arc::new(browser_surface::BrowserSurfaces::default()),
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
    home.initialize()?;
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let address = listener.local_addr()?;
    println!("CCCC Web listening on http://{address}");
    tracing::info!(%address, "CCCC Rust Web listening");
    axum::serve(listener, app(home)).await?;
    Ok(address)
}

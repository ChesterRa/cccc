mod api;
mod auth;
mod browser_surface;
mod im_runtime;
mod ledger_event_hub;
mod network;
mod notebooklm_auth;
mod readonly;
mod routes;
mod shutdown;
mod web_banner;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use rust_embed::RustEmbed;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub use readonly::WebMode;

/// Return the system browser executable used by projected browser sessions.
pub fn system_browser_path() -> Option<std::path::PathBuf> {
    browser_surface::system_browser_path()
}

const GRACEFUL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
const COMPONENT_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeOutcome {
    Stopped(SocketAddr),
    RestartRequested,
}

#[derive(Clone)]
pub(crate) struct LiveBinding {
    host: String,
    port: u16,
}

impl LiveBinding {
    fn from_env() -> Self {
        Self {
            host: std::env::var("CCCC_WEB_EFFECTIVE_HOST")
                .or_else(|_| std::env::var("CCCC_WEB_HOST"))
                .unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("CCCC_WEB_EFFECTIVE_PORT")
                .or_else(|_| std::env::var("CCCC_WEB_PORT"))
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(8848),
        }
    }
}

#[derive(Clone, Copy)]
enum RestartBehavior {
    Disabled,
    ExitProcess,
    ReturnToSupervisor,
}

#[derive(RustEmbed)]
#[folder = "$CCCC_WEB_DIST_DIR/"]
struct WebAssets;

#[derive(Clone)]
pub(crate) struct AppState {
    client: DaemonClient,
    home: HomeLayout,
    browser_surfaces: Arc<browser_surface::BrowserSurfaces>,
    notebooklm_auth: Arc<notebooklm_auth::AuthFlowManager>,
    ledger_events: ledger_event_hub::LedgerEventHub,
    im_workers: Arc<im_runtime::ImWorkerRegistry>,
    shutdown: broadcast::Sender<()>,
    restart: Option<Arc<RestartHandle>>,
    live_binding: LiveBinding,
    web_mode: WebMode,
    exhibit_allow_terminal: bool,
}

pub(crate) struct RestartHandle {
    requested: AtomicBool,
    shutdown: broadcast::Sender<()>,
}

impl RestartHandle {
    fn new(shutdown: broadcast::Sender<()>) -> Self {
        Self {
            requested: AtomicBool::new(false),
            shutdown,
        }
    }

    pub(crate) fn request(&self) -> Result<(), broadcast::error::SendError<()>> {
        self.requested.store(true, Ordering::Release);
        self.shutdown.send(()).map(|_| ())
    }

    fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

pub fn app(home: HomeLayout) -> Router {
    app_with_mode(home, WebMode::from_env())
}

pub fn app_with_mode(home: HomeLayout, web_mode: WebMode) -> Router {
    let (shutdown, _) = broadcast::channel(1);
    app_with_shutdown(home, shutdown, web_mode, None, LiveBinding::from_env()).0
}

fn app_with_shutdown(
    home: HomeLayout,
    shutdown: broadcast::Sender<()>,
    web_mode: WebMode,
    restart: Option<Arc<RestartHandle>>,
    live_binding: LiveBinding,
) -> (
    Router,
    Arc<im_runtime::ImWorkerRegistry>,
    Arc<browser_surface::BrowserSurfaces>,
    AppState,
) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let ledger_events = ledger_event_hub::LedgerEventHub::new(home.clone());
    let im_workers = Arc::new(im_runtime::ImWorkerRegistry::new(ledger_events.clone()));
    im_workers.restore_enabled(home.clone(), DaemonClient::new(home.clone()));
    let browser_surfaces = Arc::new(browser_surface::BrowserSurfaces::default());
    let notebooklm_auth = Arc::new(notebooklm_auth::AuthFlowManager::default());
    spawn_notebooklm_auth_shutdown(
        Arc::clone(&notebooklm_auth),
        Arc::clone(&browser_surfaces),
        shutdown.subscribe(),
    );
    spawn_group_resource_reaper(
        home.clone(),
        Arc::clone(&im_workers),
        Arc::clone(&browser_surfaces),
        shutdown.subscribe(),
    );
    let state = AppState {
        client: DaemonClient::new(home.clone()),
        home,
        browser_surfaces: Arc::clone(&browser_surfaces),
        notebooklm_auth,
        ledger_events,
        im_workers: Arc::clone(&im_workers),
        shutdown,
        restart,
        live_binding,
        web_mode,
        exhibit_allow_terminal: readonly::exhibit_allow_terminal_from_env(),
    };
    let app_state = state.clone();
    let app = routes::router()
        .fallback(static_asset)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            readonly::guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::authorize,
        ))
        .with_state(state);
    (app, im_workers, browser_surfaces, app_state)
}

fn spawn_notebooklm_auth_shutdown(
    auth: Arc<notebooklm_auth::AuthFlowManager>,
    browsers: Arc<browser_surface::BrowserSurfaces>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        let _ = shutdown.recv().await;
        auth.shutdown(&browsers).await;
    });
}

fn spawn_group_resource_reaper(
    home: HomeLayout,
    im_workers: Arc<im_runtime::ImWorkerRegistry>,
    browser_surfaces: Arc<browser_surface::BrowserSurfaces>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.recv() => return,
                () = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }
            let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
                continue;
            };
            let Ok(groups) = store.list() else {
                continue;
            };
            let active_groups = groups
                .iter()
                .map(|group| group.group_id.clone())
                .collect::<HashSet<_>>();
            let active_actors = groups
                .into_iter()
                .filter_map(|group| {
                    store.load(&group.group_id).ok().map(|doc| {
                        (
                            group.group_id,
                            doc.actors
                                .into_iter()
                                .map(|actor| actor.id)
                                .collect::<HashSet<_>>(),
                        )
                    })
                })
                .collect::<HashMap<_, _>>();
            let stopped = im_workers.stop_missing(&active_groups).await;
            let closed_groups = browser_surfaces
                .close_missing_groups(&active_groups)
                .await
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "failed to close stale browser surfaces");
                    0
                });
            let closed_actors = browser_surfaces
                .close_missing_actors(&active_actors)
                .await
                .unwrap_or_else(|error| {
                    tracing::warn!(%error,"failed to close stale actor browser surfaces");
                    0
                });
            let closed = closed_groups + closed_actors;
            if stopped > 0 || closed > 0 {
                tracing::info!(stopped, closed, "cleaned resources for deleted groups");
            }
        }
    });
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
    serve_until_mode(home, host, port, WebMode::from_env(), shutdown).await
}

pub async fn serve_until_mode<F>(
    home: HomeLayout,
    host: &str,
    port: u16,
    web_mode: WebMode,
    shutdown: F,
) -> Result<SocketAddr>
where
    F: Future<Output = ()> + Send + 'static,
{
    let restart_behavior = if environment_flag("CCCC_WEB_SUPERVISED") {
        RestartBehavior::ExitProcess
    } else {
        RestartBehavior::Disabled
    };
    match serve_until_mode_with_restart(home, host, port, web_mode, shutdown, restart_behavior)
        .await?
    {
        ServeOutcome::Stopped(address) => Ok(address),
        ServeOutcome::RestartRequested => unreachable!("process restart exits before returning"),
    }
}

pub async fn serve_until_mode_supervised<F>(
    home: HomeLayout,
    host: &str,
    port: u16,
    web_mode: WebMode,
    shutdown: F,
) -> Result<ServeOutcome>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_until_mode_with_restart(
        home,
        host,
        port,
        web_mode,
        shutdown,
        RestartBehavior::ReturnToSupervisor,
    )
    .await
}

async fn serve_until_mode_with_restart<F>(
    home: HomeLayout,
    host: &str,
    port: u16,
    web_mode: WebMode,
    shutdown: F,
    restart_behavior: RestartBehavior,
) -> Result<ServeOutcome>
where
    F: Future<Output = ()> + Send + 'static,
{
    home.initialize()?;
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let address = listener.local_addr()?;
    ensure_listener_auth(&home, address)?;
    web_banner::print(host, address.port());
    tracing::info!(%address, "CCCC Rust Web listening");
    let (web_shutdown, _) = broadcast::channel(1);
    let restart = (!matches!(restart_behavior, RestartBehavior::Disabled))
        .then(|| Arc::new(RestartHandle::new(web_shutdown.clone())));
    let mut restart_rx = web_shutdown.subscribe();
    let (shutdown_started, mut shutdown_started_rx) = tokio::sync::oneshot::channel();
    let (app, im_workers, browser_surfaces, app_state) = app_with_shutdown(
        home,
        web_shutdown.clone(),
        web_mode,
        restart.clone(),
        LiveBinding {
            host: host.to_owned(),
            port: address.port(),
        },
    );
    routes::spawn_web_model_supervisor(app_state);
    let server = async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = shutdown => {},
                    _ = shutdown_signal() => {},
                    _ = restart_rx.recv() => {},
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
    shutdown::browser_surfaces(&browser_surfaces).await;
    server_result?;
    if restart.as_ref().is_some_and(|handle| handle.requested()) {
        match restart_behavior {
            RestartBehavior::ExitProcess => std::process::exit(75),
            RestartBehavior::ReturnToSupervisor => return Ok(ServeOutcome::RestartRequested),
            RestartBehavior::Disabled => {}
        }
    }
    Ok(ServeOutcome::Stopped(address))
}

fn environment_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn ensure_listener_auth(home: &HomeLayout, address: SocketAddr) -> Result<()> {
    let explicitly_allowed = environment_flag("CCCC_WEB_ALLOW_UNAUTHENTICATED");
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
    fn remote_listener_requires_an_administrator_access_token() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_err());
        assert!(ensure_listener_auth(&home, "127.0.0.1:8848".parse().expect("address")).is_ok());
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("scoped", vec!["g_test".into()], false, None)
            .expect("scoped token");
        assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_err());
        AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", Vec::new(), true, None)
            .expect("admin token");
        assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_ok());
    }
    #[tokio::test]
    async fn shutdown_closes_active_sse_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let (shutdown, _) = broadcast::channel(1);
        let response = app_with_shutdown(
            home,
            shutdown.clone(),
            WebMode::Normal,
            None,
            LiveBinding::from_env(),
        )
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

    #[tokio::test]
    async fn shutdown_closes_headless_sse_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let group = store.create("headless shutdown", "").expect("group");
        let events = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl");
        std::fs::create_dir_all(events.parent().expect("events parent")).expect("headless dir");
        std::fs::write(&events, "").expect("events file");
        let (shutdown, _) = broadcast::channel(1);
        let response = app_with_shutdown(
            home,
            shutdown.clone(),
            WebMode::Normal,
            None,
            LiveBinding::from_env(),
        )
        .0
        .oneshot(
            axum::http::Request::builder()
                .uri(format!(
                    "/api/v1/groups/{}/headless/stream?replay=false",
                    group.group_id
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("headless SSE response");
        let mut body = response.into_body().into_data_stream();
        shutdown.send(()).expect("active headless SSE subscriber");
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
                .await
                .expect("headless SSE shutdown timeout")
                .is_none()
        );
    }
}

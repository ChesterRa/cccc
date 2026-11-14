//! This route uses signed device keys and pairing capabilities, never browser cookies.
use super::relay_copy;
use axum::{
    Router,
    extract::{OriginalUri, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use cccc_core::{HomeLayout, session_nodes};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, broadcast};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

#[derive(Clone)]
struct Proxy {
    home: HomeLayout,
    read_only: bool,
    shutdown: broadcast::Sender<()>,
    permits: Arc<Semaphore>,
    checks: Arc<Semaphore>,
}

pub(crate) fn routes(home: HomeLayout, read_only: bool, shutdown: broadcast::Sender<()>) -> Router {
    Router::new()
        .route("/relay", get(upgrade))
        .route("/ping", get(probe))
        .route("/generate_204", get(probe))
        .with_state(Proxy {
            home,
            read_only,
            shutdown,
            permits: Arc::new(Semaphore::new(64)),
            checks: Arc::new(Semaphore::new(2)),
        })
}

async fn upgrade(
    State(state): State<Proxy>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let shutdown = state.shutdown.subscribe();
    if state.read_only {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = state.permits.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(address) = local_address(&state).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let path = uri
        .path_and_query()
        .map_or("/relay", |value| value.as_str());
    let Ok(mut request) = format!("ws://{address}{path}").into_client_request() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // TLS-exporter authentication belongs to the edge TLS session. The local
    // relay must use iroh's challenge handshake, not an edge-bound proof.
    for name in [header::AUTHORIZATION, header::SEC_WEBSOCKET_PROTOCOL] {
        if let Some(value) = headers.get(&name) {
            let mut value = value.clone();
            if name == header::AUTHORIZATION {
                value.set_sensitive(true);
            }
            request.headers_mut().insert(name, value);
        }
    }
    let upstream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(request),
    )
    .await;
    let Ok(Ok((upstream, response))) = upstream else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let protocol = response
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let ws = if let Some(protocol) = protocol {
        ws.protocols([protocol])
    } else {
        ws
    };
    let mut response = ws
        .max_message_size(128 * 1024)
        .max_frame_size(128 * 1024)
        .on_upgrade(move |edge| async move {
            let _permit = permit;
            relay_copy::copy(edge, upstream, shutdown).await;
        });
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("static cache policy"),
    );
    response
}

async fn probe(
    State(state): State<Proxy>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    if state.read_only {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(_permit) = state.permits.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if local_address(&state).await.is_none() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    super::relay_probe::response(uri.path(), &headers).into_response()
}

async fn local_address(state: &Proxy) -> Option<std::net::SocketAddr> {
    let _check = state.checks.clone().try_acquire_owned().ok()?;
    let home = state.home.clone();
    tokio::task::spawn_blocking(move || {
        if session_nodes::relay_public_url(&home)?.is_none() {
            return Ok(None);
        }
        session_nodes::relay_address(&home)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .flatten()
}

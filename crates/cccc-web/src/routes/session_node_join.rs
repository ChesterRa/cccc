//! Compatibility tombstones: no body parsing, queue, ticket issuance or links.
use crate::{
    AppState,
    api::{ApiError, ApiResult},
};
use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session_nodes/join/requests", post(retired))
        .route("/api/v1/session_nodes/join/poll", post(retired))
        .route("/api/v1/session_nodes/join/pending", get(retired))
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}",
            get(retired),
        )
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}/approve",
            post(retired),
        )
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}/deny",
            post(retired),
        )
        .layer(middleware::from_fn(no_store))
}

async fn retired() -> ApiResult {
    Err(ApiError::not_found_code(
        "session_node_join_retired",
        "Browser device approval has been retired; use cccc sessions node connect --ticket-stdin with a pairing ticket",
    ))
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

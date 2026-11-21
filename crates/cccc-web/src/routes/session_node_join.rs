//! Only enrollment creation and secret-bearing polling are public.
use super::session_nodes::{authorize, unavailable};
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
    auth::Principal,
    session_node,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Extension, Path, State, rejection::JsonRejection},
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use cccc_contracts::session_node_join::{CreateRequest, PollRequest};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session_nodes/join/requests", post(create))
        .route("/api/v1/session_nodes/join/poll", post(poll))
        .route("/api/v1/session_nodes/join/pending", get(pending))
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}",
            get(detail),
        )
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}/approve",
            post(approve),
        )
        .route(
            "/api/v1/session_nodes/join/requests/{request_id}/deny",
            post(deny),
        )
        .layer(DefaultBodyLimit::max(1024))
        .layer(middleware::from_fn(no_store))
}

async fn no_store(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn writable(state: &AppState) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Device enrollment is unavailable in read-only Web",
        ));
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> ApiResult {
    writable(&state)?;
    let Json(input) = body.map_err(|_| ApiError::bad("Invalid device enrollment request"))?;
    let result = state
        .session_node_join
        .lock()
        .await
        .create(input, &state.home)?;
    Ok(success(json!(result)))
}

async fn poll(
    State(state): State<AppState>,
    body: Result<Json<PollRequest>, JsonRejection>,
) -> ApiResult {
    writable(&state)?;
    let Json(input) = body.map_err(|_| ApiError::bad("Invalid device enrollment request"))?;
    let result = state
        .session_node_join
        .lock()
        .await
        .poll(input, &state.home)?;
    Ok(success(json!(result)))
}

async fn pending(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut queue = state.session_node_join.lock().await;
    authorize(&state, &principal)?;
    let requests = queue.pending(&state.home)?;
    Ok(success(json!({"requests":requests})))
}

async fn detail(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut queue = state.session_node_join.lock().await;
    authorize(&state, &principal)?;
    let request = queue.detail(&id, &state.home)?;
    Ok(success(json!({"request":request})))
}

async fn approve(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    // Serialize approval/denial/revocation with publication of the one ticket.
    let mut queue = state.session_node_join.lock().await;
    authorize(&state, &principal)?;
    if let Some(request) = queue.approval(&id, &state.home)? {
        let ticket = session_node::create_bound_invitation(
            &state.home,
            &request.node_id,
            &request.expires_at,
            &queue.generation(&id)?,
        )
        .await
        .map_err(unavailable)?;
        let commit = authorize(&state, &principal).and_then(|_| {
            let encoded = session_node::encode_ticket(&ticket).map_err(unavailable)?;
            queue.approve(&id, encoded, &state.home)
        });
        if let Err(error) = commit {
            let _ = cccc_core::session_nodes::discard_invite(
                &state.home,
                &ticket.invite_id,
                &ticket.secret,
            );
            return Err(error);
        }
    }
    Ok(success(json!({"status":"approved"})))
}

async fn deny(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut queue = state.session_node_join.lock().await;
    authorize(&state, &principal)?;
    queue.deny(&id, &state.home)?;
    Ok(success(json!({"status":"denied"})))
}

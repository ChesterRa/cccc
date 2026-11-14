//! Administrator-only device pairing, separate from Group invitation links.
use crate::{
    AppState,
    api::{ApiError, ApiResult},
    auth::Principal,
    session_node,
};
use axum::{
    Json, Router,
    extract::{Extension, Path, State},
    routing::{get, post},
};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/session_nodes", get(status))
        .route("/api/v1/session_nodes/invitations", post(invite))
        .route("/api/v1/session_nodes/{node_id}/revoke", post(revoke))
}

fn authorize(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Session-node pairing requires this instance's administrator",
        ));
    }
    Ok(())
}

async fn status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let mut endpoint = cccc_core::session_nodes::endpoint(&state.home).map_err(unavailable)?;
    if let Some(endpoint) = &mut endpoint
        && cccc_core::session_nodes::relay_address(&state.home)
            .map_err(unavailable)?
            .is_some()
    {
        endpoint.relay_url =
            cccc_core::session_nodes::relay_public_url(&state.home).map_err(unavailable)?;
    }
    let devices = cccc_core::session_nodes::peers(&state.home).map_err(unavailable)?;
    Ok(Json(
        json!({"ok":true,"result":{"supported":true,"transport":"iroh","endpoint":endpoint,"devices":devices}}),
    ))
}

async fn invite(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let ticket = session_node::create_invitation(&state.home, 0)
        .await
        .map_err(unavailable)?;
    authorize(&state, &principal)?;
    let encoded = session_node::encode_ticket(&ticket).map_err(unavailable)?;
    Ok(Json(
        json!({"ok":true,"result":{"ticket":encoded,"expires_at":ticket.expires_at}}),
    ))
}

async fn revoke(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(node_id): Path<String>,
) -> ApiResult {
    authorize(&state, &principal)?;
    session_node::revoke(&state.home, &node_id)
        .await
        .map_err(unavailable)?;
    Ok(Json(json!({"ok":true,"result":{"revoked":true}})))
}

fn unavailable(error: impl std::fmt::Display) -> ApiError {
    ApiError::unavailable("session_node_unavailable", error.to_string())
}

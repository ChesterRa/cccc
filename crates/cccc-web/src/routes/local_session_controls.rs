//! Explicit administrator operations; never accept paths or forwarded identities.
use crate::{
    AppState,
    api::{ApiError, ApiResult, call, object},
    auth::Principal,
};
use axum::{
    Json, Router,
    extract::{Extension, State},
    routing::post,
};
use serde_json::{Value, json};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/local_sessions/control", post(control))
        .route("/api/v1/local_sessions/recheck", post(recheck))
}

fn authorize(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Source management requires this instance's administrator",
        ));
    }
    Ok(())
}

async fn control(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(&state,"local_session_control",object(json!({"source_id":body["source_id"],"action":body["action"],"expected_revision":body["expected_revision"],"by":"user"}))).await?;
    authorize(&state, &principal)?;
    Ok(response)
}

async fn recheck(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(
        &state,
        "local_session_recheck",
        object(json!({"source_id":body["source_id"],"by":"user"})),
    )
    .await?;
    authorize(&state, &principal)?;
    Ok(response)
}

//! Private, administrator-only directory of this instance's user-owned sessions.
use crate::{
    AppState,
    api::{ApiError, ApiResult, body_object, call, object},
    auth::Principal,
};
use axum::{
    Json, Router,
    extract::{Extension, Path, State},
    routing::{get, post},
};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/local_sessions", get(list))
        .route(
            "/api/v1/local_sessions/recovery/{group_id}/{actor_id}",
            get(recovery),
        )
        .route(
            "/api/v1/local_sessions/recovery/{group_id}/{actor_id}/refresh",
            post(refresh),
        )
        .route("/api/v1/local_sessions/conversations", post(conversation))
        .route("/api/v1/local_sessions/join", post(join))
}
fn authorize(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Local sessions are available only to this instance's administrator",
        ));
    }
    Ok(())
}
async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    authorize(&state, &principal)?;
    call(
        &state,
        "local_session_list",
        object(json!({"include_offline":true})),
    )
    .await
}

async fn recovery(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(
        &state,
        "local_session_recovery",
        object(json!({"group_id":group_id,"actor_id":actor_id,"by":"user"})),
    )
    .await?;
    authorize(&state, &principal)?;
    Ok(response)
}

async fn refresh(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let response = call(
        &state,
        "local_session_refresh",
        object(json!({"group_id":group_id,"actor_id":actor_id,"by":"user"})),
    )
    .await?;
    authorize(&state, &principal)?;
    Ok(response)
}

async fn conversation(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let body = body_object(body)?;
    // Web creates a human conversation, never impersonates a discovered source.
    let args = body
        .into_iter()
        .filter(|(key, _)| matches!(key.as_str(), "source_ids" | "title" | "direct"))
        .collect();
    call(&state, "local_session_conversation", args).await
}

async fn join(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    authorize(&state, &principal)?;
    let args = body_object(body)?
        .into_iter()
        .filter(|(key, _)| matches!(key.as_str(), "group_id" | "source_ids"))
        .collect();
    call(&state, "local_session_join", args).await
}

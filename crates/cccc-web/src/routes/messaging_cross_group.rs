use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call};
use crate::auth::Principal;
use axum::extract::{DefaultBodyLimit, Extension, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/send_cross_group",
            post(send_json),
        )
        // Keep the existing local JSON limit when retiring the remote upload route.
        .layer(DefaultBodyLimit::max(11 * 1024 * 1024))
}
async fn send_json(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let destination = required(&body, "dst_group_id")?;
    ensure_access(&principal, &destination)?;
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "send_cross_group", args).await
}

pub(super) fn ensure_access(principal: &Principal, group_id: &str) -> Result<(), ApiError> {
    principal
        .allows(group_id)
        .then_some(())
        .ok_or_else(|| ApiError::forbidden("group access denied"))
}

pub(super) fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

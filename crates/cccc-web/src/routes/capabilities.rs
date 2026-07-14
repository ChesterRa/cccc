use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/capabilities/overview", get(overview))
        .route(
            "/api/v1/capabilities/allowlist",
            get(allowlist_get)
                .put(allowlist_update)
                .delete(allowlist_reset),
        )
        .route(
            "/api/v1/capabilities/allowlist/validate",
            post(allowlist_validate),
        )
        .route("/api/v1/capabilities/block", post(block))
        .route("/api/v1/groups/{group_id}/capabilities/state", get(state))
        .route(
            "/api/v1/groups/{group_id}/capabilities/enable",
            post(enable),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/visibility",
            post(visibility),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/use",
            post(use_capability),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/import",
            post(import),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/install",
            post(install),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/sources/delete",
            post(source_delete),
        )
        .route(
            "/api/v1/groups/{group_id}/capabilities/uninstall",
            post(uninstall),
        )
}

async fn overview(State(state): State<AppState>) -> ApiResult {
    call(&state, "capability_overview", Default::default()).await
}
async fn allowlist_get(State(state): State<AppState>) -> ApiResult {
    call(&state, "capability_allowlist_get", Default::default()).await
}
async fn allowlist_update(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "capability_allowlist_update", body_object(body)?).await
}
async fn allowlist_reset(State(state): State<AppState>) -> ApiResult {
    call(&state, "capability_allowlist_reset", Default::default()).await
}
async fn allowlist_validate(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "capability_allowlist_validate", body_object(body)?).await
}
async fn block(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "capability_block", body_object(body)?).await
}
async fn state(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "capability_state",
        object(json!({"group_id":group_id})),
    )
    .await
}
async fn enable(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_enable", group_id, body).await
}
async fn visibility(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_visibility", group_id, body).await
}
async fn use_capability(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_tool_call", group_id, body).await
}
async fn import(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_import", group_id, body).await
}
async fn uninstall(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_uninstall", group_id, body).await
}
async fn install(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_install", group_id, body).await
}
async fn source_delete(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    with_group(&state, "capability_source_delete", group_id, body).await
}
async fn with_group(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(state, op, args).await
}

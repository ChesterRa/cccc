use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
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

#[derive(Default, Deserialize, Serialize)]
struct OverviewQuery {
    query: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    include_indexed: Option<bool>,
    include_source_instances: Option<bool>,
    kind: Option<String>,
    policy: Option<String>,
    source_id: Option<String>,
    group_id: Option<String>,
}

async fn overview(State(state): State<AppState>, Query(query): Query<OverviewQuery>) -> ApiResult {
    let args = serde_json::to_value(query)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    call(&state, "capability_overview", args).await
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
#[derive(Default, Deserialize)]
struct StateQuery {
    actor_id: Option<String>,
    view: Option<String>,
    capability_id: Option<String>,
}

async fn state(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<StateQuery>,
) -> ApiResult {
    let mut args = object(json!({"group_id":group_id}));
    if let Some(actor_id) = query.actor_id {
        args.insert("actor_id".into(), json!(actor_id));
    }
    if let Some(view) = query.view {
        args.insert("view".into(), json!(view));
    }
    if let Some(capability_id) = query.capability_id {
        args.insert("capability_id".into(), json!(capability_id));
    }
    call(&state, "capability_state", args).await
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
    with_group(&state, "capability_install_target", group_id, body).await
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

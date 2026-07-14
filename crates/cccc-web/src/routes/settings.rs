use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/settings",
            get(settings_get).put(settings_update),
        )
        .route(
            "/api/v1/groups/{group_id}/automation",
            get(automation_get).put(automation_update),
        )
        .route(
            "/api/v1/groups/{group_id}/automation/manage",
            post(automation_manage),
        )
        .route(
            "/api/v1/groups/{group_id}/automation/reset_baseline",
            post(automation_reset),
        )
}

async fn settings_get(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    let mut response = call(&state, "group_show", object(json!({"group_id":group_id}))).await?;
    let settings = response
        .0
        .get("result")
        .and_then(|result| result.get("group"))
        .and_then(|group| group.get("settings"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    response.0 = json!({"ok":true,"result":{"settings":settings}});
    Ok(response)
}
async fn settings_update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "group_settings_update",
        object(json!({"group_id":group_id,"patch":body,"by":"user"})),
    )
    .await
}
async fn automation_get(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "group_automation_state",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}
async fn automation_update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "group_automation_update",
        object(json!({"group_id":group_id,"patch":body,"by":"user"})),
    )
    .await
}
async fn automation_manage(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "group_automation_manage", args).await
}
async fn automation_reset(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> ApiResult {
    call(
        &state,
        "group_automation_reset_baseline",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}

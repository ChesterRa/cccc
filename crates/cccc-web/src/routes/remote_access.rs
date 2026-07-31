use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/remote_access", get(state).put(configure))
        .route("/api/v1/remote_access/start", post(start))
        .route("/api/v1/remote_access/stop", post(stop))
        .route("/api/v1/remote_access/apply", post(apply))
}

async fn state(State(state): State<AppState>) -> ApiResult {
    let mut response = call(&state, "remote_access_state", Map::new()).await?;
    super::remote_access_projection::apply(&state, &mut response.0["result"]["remote_access"]);
    Ok(response)
}

async fn configure(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let mut response = call(&state, "remote_access_configure", body_object(body)?).await?;
    super::remote_access_projection::apply(&state, &mut response.0["result"]["remote_access"]);
    Ok(response)
}

async fn start(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "remote_access_start",
        object(json!({"by":query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn stop(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "remote_access_stop",
        object(json!({"by":query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn apply(State(state): State<AppState>) -> ApiResult {
    let response = call(&state, "remote_access_state", Map::new()).await?;
    let mut remote = response.0["result"]["remote_access"].clone();
    super::remote_access_projection::apply(&state, &mut remote);
    if !remote
        .get("restart_required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(Json(json!({"ok":true,"result":{
            "accepted":false,
            "remote_access":remote
        }})));
    }
    if !remote
        .get("apply_supported")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(ApiError::conflict(
            "web_apply_unsupported",
            "the running Web service is not supervisor-managed, so it cannot self-apply binding changes",
            json!({}),
        ));
    }
    let restart = state.restart.as_ref().ok_or_else(|| {
        ApiError::conflict(
            "web_apply_unavailable",
            "web apply is not available in this runtime",
            json!({}),
        )
    })?;
    restart.request().map_err(|error| {
        ApiError::unavailable(
            "web_apply_failed",
            format!("failed to request restart: {error}"),
        )
    })?;
    let diagnostics = remote.get("diagnostics").cloned().unwrap_or_default();
    Ok(Json(json!({"ok":true,"result":{
        "accepted":true,
        "target_local_url":diagnostics.get("desired_local_url").cloned().unwrap_or(Value::Null),
        "target_remote_url":diagnostics.get("desired_remote_url").cloned().unwrap_or(Value::Null),
        "remote_access":remote
    }})))
}

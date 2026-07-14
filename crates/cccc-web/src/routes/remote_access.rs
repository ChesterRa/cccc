use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/remote_access", get(state).put(configure))
        .route("/api/v1/remote_access/start", post(start))
        .route("/api/v1/remote_access/stop", post(stop))
        .route("/api/v1/remote_access/apply", post(apply))
}

async fn state(State(state): State<AppState>) -> ApiResult {
    call(&state, "remote_access_state", Map::new()).await
}

async fn configure(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "remote_access_configure", body_object(body)?).await
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
    let remote = response.0["result"]["remote_access"].clone();
    Ok(Json(json!({"ok":true,"result":{
        "accepted":true,
        "target_local_url":remote.get("config").map(|config| format!("http://{}:{}", config["web_host"].as_str().unwrap_or("127.0.0.1"), config["web_port"].as_u64().unwrap_or(8848))),
        "target_remote_url":remote.get("endpoint").cloned().unwrap_or(Value::Null),
        "remote_access":remote
    }})))
}

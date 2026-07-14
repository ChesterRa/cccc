use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/debug/snapshot", get(snapshot))
        .route("/api/v1/debug/tail_logs", get(tail_logs))
        .route("/api/v1/debug/clear_logs", post(clear_logs))
}

async fn snapshot(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "debug_snapshot",
        object(json!({"group_id":query.get("group_id").cloned().unwrap_or_default(),"by":"user"})),
    )
    .await
}

async fn tail_logs(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let mut args = object(json!({
        "component":query.get("component").cloned().unwrap_or_default(),
        "group_id":query.get("group_id").cloned().unwrap_or_default(),
        "by":"user"
    }));
    if let Some(lines) = query
        .get("lines")
        .and_then(|value| value.parse::<u64>().ok())
    {
        args.insert("lines".into(), Value::Number(lines.into()));
    }
    call(&state, "debug_tail_logs", args).await
}

async fn clear_logs(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(&state, "debug_clear_logs", body_object(body)?).await
}

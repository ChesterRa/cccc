use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object, success};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/context",
            get(context_get).post(context_sync),
        )
        .route("/api/v1/groups/{group_id}/tasks", get(tasks))
        .route("/api/v1/groups/{group_id}/ledger/tail", get(ledger_tail))
        .route(
            "/api/v1/groups/{group_id}/ledger/search",
            get(ledger_search),
        )
        .route("/api/v1/groups/{group_id}/ledger/window", get(ledger_tail))
        .route(
            "/api/v1/groups/{group_id}/ledger/statuses",
            post(ledger_statuses),
        )
        .route(
            "/api/v1/groups/{group_id}/events/{event_id}/read_status",
            get(read_status),
        )
}

async fn context_get(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(&state, "context_get", object(json!({"group_id":group_id}))).await
}
async fn context_sync(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "context_sync", args).await
}
async fn tasks(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let mut args = object(json!({"group_id":group_id}));
    if let Some(status) = query.get("status") {
        args.insert("status".into(), Value::String(status.clone()));
    }
    call(&state, "task_list", args).await
}
async fn ledger_tail(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let limit = query
        .get("limit")
        .or_else(|| query.get("n"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(200);
    call(
        &state,
        "ledger_tail",
        object(json!({"group_id":group_id,"limit":limit})),
    )
    .await
}
async fn ledger_search(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let term = query
        .get("q")
        .or_else(|| query.get("query"))
        .cloned()
        .unwrap_or_default()
        .to_lowercase();
    let mut response = call(
        &state,
        "ledger_tail",
        object(json!({"group_id":group_id,"limit":1000})),
    )
    .await?;
    let matches: Vec<_> = response
        .0
        .get("result")
        .and_then(|result| result.get("events"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|event| {
            serde_json::to_string(event)
                .unwrap_or_default()
                .to_lowercase()
                .contains(&term)
        })
        .cloned()
        .collect();
    response.0 = json!({"ok":true,"result":{"events":matches,"matches":matches}});
    Ok(response)
}
async fn ledger_statuses(Path(group_id): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    let ids = body.get("event_ids").cloned().unwrap_or_else(|| json!([]));
    success(json!({"group_id":group_id,"statuses":{},"event_ids":ids}))
}
async fn read_status(Path((group_id, event_id)): Path<(String, String)>) -> Json<Value> {
    success(
        json!({"group_id":group_id,"event_id":event_id,"read_by":[],"acked_by":[],"replied_by":[]}),
    )
}

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

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
        .route(
            "/api/v1/groups/{group_id}/ledger/window",
            get(ledger_window),
        )
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
    let limit = first_non_blank_query(&query, &["limit", "n"])
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(200);
    call(
        &state,
        "ledger_tail",
        object(json!({"group_id":group_id,"limit":limit,"kind":query.get("kind")})),
    )
    .await
}
async fn ledger_search(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "ledger_search",
        object(json!({
            "group_id":group_id,
            "q":first_non_blank_query(&query,&["q","query"]),
            "kind":query.get("kind"),
            "by":query.get("by"),
            "before":query.get("before"),
            "after":query.get("after"),
            "limit":query.get("limit"),
        })),
    )
    .await
}

fn first_non_blank_query<'a>(
    query: &'a HashMap<String, String>,
    names: &[&str],
) -> Option<&'a str> {
    names.iter().find_map(|name| {
        query
            .get(*name)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}
async fn ledger_window(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "ledger_window",
        object(json!({
            "group_id":group_id,
            "center":query.get("center"),
            "kind":query.get("kind"),
            "before":query.get("before"),
            "after":query.get("after"),
        })),
    )
    .await
}
async fn ledger_statuses(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    call(
        &state,
        "ledger_statuses",
        object(json!({"group_id":group_id,"event_ids":body.get("event_ids")})),
    )
    .await
}
async fn read_status(
    State(state): State<AppState>,
    Path((group_id, event_id)): Path<(String, String)>,
) -> ApiResult {
    call(
        &state,
        "message_read_status",
        object(json!({"group_id":group_id,"event_id":event_id})),
    )
    .await
}

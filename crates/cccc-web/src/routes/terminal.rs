use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

#[derive(Debug, Deserialize)]
struct TerminalQuery {
    actor_id: String,
    before: Option<String>,
    max_chars: Option<u64>,
    limit_bytes: Option<u64>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/terminal/tail", get(tail))
        .route("/api/v1/groups/{group_id}/terminal/history", get(history))
        .route(
            "/api/v1/groups/{group_id}/terminal/write",
            axum::routing::post(write),
        )
        .route(
            "/api/v1/groups/{group_id}/terminal/resize",
            axum::routing::post(resize),
        )
        .route(
            "/api/v1/groups/{group_id}/terminal/clear",
            axum::routing::post(clear),
        )
        .merge(super::terminal_ws::routes())
}

async fn tail(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<TerminalQuery>,
) -> ApiResult {
    call(
        &state,
        "terminal_tail",
        object(json!({
            "group_id": group_id,
            "actor_id": query.actor_id,
            "max_chars": query.max_chars.unwrap_or(8_000),
            "by": "user",
        })),
    )
    .await
}

async fn history(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<TerminalQuery>,
) -> ApiResult {
    call(
        &state,
        "terminal_history",
        object(json!({
            "group_id": group_id,
            "actor_id": query.actor_id,
            "before": query.before,
            "limit_bytes": query.limit_bytes.unwrap_or(64_000),
            "by": "user",
        })),
    )
    .await
}

async fn write(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    command(&state, "terminal_write", group_id, body).await
}

async fn resize(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    command(&state, "terminal_resize", group_id, body).await
}

async fn clear(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    command(&state, "terminal_clear", group_id, body).await
}

async fn command(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    call(state, op, args).await
}

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
    render_before: Option<u64>,
    max_chars: Option<u64>,
    limit_bytes: Option<u64>,
    #[serde(default)]
    strip_ansi: bool,
    #[serde(default)]
    compact: bool,
}

#[derive(Debug, Deserialize)]
struct TerminalClearQuery {
    actor_id: String,
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
            "strip_ansi": query.strip_ansi,
            "compact": query.compact,
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
            "render_before": query.render_before,
            "limit_bytes": query.limit_bytes.unwrap_or(64_000),
            "strip_ansi": query.strip_ansi,
            "compact": query.compact,
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
    command(&state, "term_resize", group_id, body).await
}

async fn clear(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<TerminalClearQuery>,
) -> ApiResult {
    call(
        &state,
        "terminal_clear",
        object(json!({
            "group_id": group_id,
            "actor_id": query.actor_id,
            "by": "user",
        })),
    )
    .await
}

async fn command(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    call(state, op, args).await
}

#[cfg(test)]
mod tests {
    use super::TerminalQuery;

    #[test]
    fn terminal_query_accepts_tail_rendering_options() {
        let query: TerminalQuery = serde_json::from_value(serde_json::json!({
            "actor_id": "peer1",
            "strip_ansi": true,
            "compact": true,
            "render_before": 128_000,
        }))
        .expect("terminal query");

        assert!(query.strip_ansi);
        assert!(query.compact);
        assert_eq!(query.render_before, Some(128_000));
    }
}

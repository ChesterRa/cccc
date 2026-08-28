use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

#[derive(Debug, Deserialize)]
struct SpaceQuery {
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default)]
    lane: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    state: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/space/status", get(status))
        .route("/api/v1/groups/{group_id}/space/spaces", get(spaces))
        .route("/api/v1/groups/{group_id}/space/bind", post(bind))
        .route("/api/v1/groups/{group_id}/space/ingest", post(ingest))
        .route("/api/v1/groups/{group_id}/space/query", post(query))
        .route(
            "/api/v1/groups/{group_id}/space/sources",
            get(list_sources).post(source_action),
        )
        .route(
            "/api/v1/groups/{group_id}/space/artifacts",
            get(list_artifacts).post(artifact_action),
        )
        .route(
            "/api/v1/groups/{group_id}/space/jobs",
            get(list_jobs).post(job_action),
        )
}

async fn status(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    daemon_get(&state, "group_space_status", group_id, query, false).await
}

async fn spaces(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    daemon_get(&state, "group_space_spaces", group_id, query, false).await
}

async fn list_sources(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    daemon_get(&state, "group_space_sources", group_id, query, true).await
}

async fn list_artifacts(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    daemon_get(&state, "group_space_artifact", group_id, query, true).await
}

async fn list_jobs(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    daemon_get(&state, "group_space_jobs", group_id, query, true).await
}

async fn bind(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_bind", group_id, body).await
}

async fn ingest(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_ingest", group_id, body).await
}

async fn query(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_query", group_id, body).await
}

async fn source_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_sources", group_id, body).await
}

async fn artifact_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_artifact", group_id, body).await
}

async fn job_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "group_space_jobs", group_id, body).await
}

async fn daemon_get(
    state: &AppState,
    op: &str,
    group_id: String,
    query: SpaceQuery,
    include_action: bool,
) -> ApiResult {
    let mut args = object(json!({
        "group_id": group_id,
        "provider": query.provider,
        "lane": query.lane,
        "kind": query.kind,
        "state": query.state,
        "limit": query.limit,
        "by": "user",
    }));
    if include_action {
        args.insert("action".into(), Value::String("list".into()));
    }
    call(state, op, args).await
}

async fn daemon_body(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    let mut args: Map<String, Value> = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.entry("by")
        .or_insert_with(|| Value::String("user".into()));
    call(state, op, args).await
}

fn default_provider() -> String {
    "notebooklm".into()
}
fn default_limit() -> usize {
    100
}

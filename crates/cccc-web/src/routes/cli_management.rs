use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/cli-management", get(status))
        .route("/api/v1/cli-management/jobs", post(submit))
        .route("/api/v1/cli-management/jobs/{job_id}/log", get(read_log))
        .route(
            "/api/v1/cli-management/schedules",
            axum::routing::put(save_schedules),
        )
        .route("/api/v1/cli-management/uninstall", post(uninstall))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LogQuery {
    #[serde(default)]
    offset: u64,
}

async fn read_log(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Query(query): Query<LogQuery>,
) -> ApiResult {
    call(
        &state,
        "cli_management_log",
        object(json!({"by":"user","job_id":job_id,"offset":query.offset})),
    )
    .await
}

async fn status(State(state): State<AppState>) -> ApiResult {
    call(&state, "cli_management_get", object(json!({"by":"user"}))).await
}

async fn submit(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    forward(&state, "cli_management_submit", body).await
}

async fn save_schedules(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    forward(&state, "cli_management_schedules_update", body).await
}

async fn uninstall(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    forward(&state, "cli_management_uninstall", body).await
}

async fn forward(state: &AppState, op: &str, body: Value) -> ApiResult {
    let mut args = body_object(body)?;
    // 全局管理员身份已在统一 HTTP 授权边界检查；客户端不能提供另一种控制身份。
    args.insert("by".into(), json!("user"));
    call(state, op, args).await
}

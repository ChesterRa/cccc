//! Thin ports; task ownership and process lifetime belong to the daemon.
use crate::{
    AppState,
    api::{ApiError, ApiResult, call, object},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/voice-secretary/runtime", get(runtime))
        .route("/api/v1/voice-secretary/runtime/reset", post(reset_runtime))
        .route(
            "/api/v1/voice-secretary/settings",
            get(settings).put(update_settings),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/tasks",
            get(tasks),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/tasks/{task_id}/cancel",
            post(cancel),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/tasks/{task_id}/retry",
            post(retry),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/tasks/{task_id}/candidate",
            get(candidate),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/tasks/{task_id}/handoff",
            post(handoff),
        )
        .merge(super::voice_secretary_terminal::routes())
}
async fn settings(State(state): State<AppState>) -> ApiResult {
    call(
        &state,
        "voice_secretary_settings_get",
        object(json!({"by":"user"})),
    )
    .await
}
async fn update_settings(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let mut args = object(body);
    if let Some(command) = args
        .get_mut("settings")
        .and_then(|settings| settings.get_mut("command"))
        && let Some(command_line) = command.as_str()
    {
        *command = json!(shell_words::split(command_line).map_err(|error| {
            ApiError::bad_code(
                "invalid_args",
                format!("invalid runtime command: {error}"),
                json!({}),
            )
        })?);
    }
    args.insert("by".into(), json!("user"));
    call(&state, "voice_secretary_settings_update", args).await
}
async fn tasks(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "voice_secretary_tasks",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}
async fn task_call(
    state: AppState,
    group_id: String,
    task_id: String,
    body: Value,
    op: &str,
) -> ApiResult {
    let mut args = object(body);
    args.insert("by".into(), json!("user"));
    args.insert("group_id".into(), json!(group_id));
    args.insert("task_id".into(), json!(task_id));
    call(&state, op, args).await
}
async fn cancel(
    State(state): State<AppState>,
    Path((group, task)): Path<(String, String)>,
) -> ApiResult {
    task_call(state, group, task, json!({}), "voice_secretary_task_cancel").await
}
async fn retry(
    State(state): State<AppState>,
    Path((group, task)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    task_call(state, group, task, body, "voice_secretary_task_retry").await
}
async fn candidate(
    State(state): State<AppState>,
    Path((group, task)): Path<(String, String)>,
) -> ApiResult {
    task_call(
        state,
        group,
        task,
        json!({}),
        "voice_secretary_task_candidate",
    )
    .await
}
async fn handoff(
    State(state): State<AppState>,
    Path((group, task)): Path<(String, String)>,
) -> ApiResult {
    task_call(
        state,
        group,
        task,
        json!({}),
        "voice_secretary_task_handoff",
    )
    .await
}

async fn runtime(State(state): State<AppState>) -> ApiResult {
    call(
        &state,
        "voice_secretary_runtime",
        object(json!({"by":"user"})),
    )
    .await
}
async fn reset_runtime(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let mut args = object(body);
    args.insert("by".into(), json!("user"));
    call(&state, "voice_secretary_runtime_reset", args).await
}

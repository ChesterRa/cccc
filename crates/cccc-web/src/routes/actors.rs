use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiResult, body_object, call, object};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/actors", get(list).post(create))
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}",
            post(update).delete(remove),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/start",
            post(start),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/stop",
            post(stop),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/restart",
            post(restart),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/new_session",
            post(new_session),
        )
        .merge(super::actor_assets::routes())
        .merge(super::actor_profiles::routes())
}

#[derive(Default, serde::Deserialize)]
struct ActorListQuery {
    #[serde(default)]
    include_unread: bool,
    #[serde(default)]
    include_internal: bool,
}

async fn list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ActorListQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_list",
        object(json!({
            "group_id":group_id,
            "by":"user",
            "include_unread":query.include_unread,
            "include_internal":query.include_internal,
        })),
    )
    .await
}
async fn create(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    normalize_command(&mut args)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "actor_add", args).await
}
async fn update(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    normalize_command(&mut args)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("actor_id".into(), Value::String(actor_id));
    call(&state, "actor_update", args).await
}
async fn remove(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    let mut response = lifecycle(&state, &group_id, &actor_id, "actor_remove").await?;
    let key = format!("web-model::{group_id}::{actor_id}");
    if let Err(error) = state.browser_surfaces.close(&key).await {
        response.0["result"]["browser_cleanup_warning"] = Value::String(error.to_string());
    }
    Ok(response)
}
async fn start(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_start").await
}
async fn stop(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_stop").await
}
async fn restart(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_restart").await
}
async fn new_session(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_new_session").await
}
async fn lifecycle(state: &AppState, group_id: &str, actor_id: &str, op: &str) -> ApiResult {
    call(
        state,
        op,
        object(json!({"group_id":group_id,"actor_id":actor_id,"by":"user"})),
    )
    .await
}

fn normalize_command(args: &mut Map<String, Value>) -> Result<(), crate::api::ApiError> {
    if let Some(Value::String(command)) = args.get("command").cloned() {
        let command = shell_words::split(&command).map_err(|error| {
            crate::api::ApiError::bad_code("invalid_command", error.to_string(), json!({}))
        })?;
        args.insert("command".into(), json!(command));
    }
    Ok(())
}

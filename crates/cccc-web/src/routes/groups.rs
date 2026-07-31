use axum::extract::{Extension, Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups", get(list).post(create))
        .route(
            "/api/v1/groups/{group_id}",
            get(show).put(update).delete(remove),
        )
        .route("/api/v1/groups/{group_id}/reset", post(reset))
        .route("/api/v1/groups/{group_id}/start", post(start))
        .route("/api/v1/groups/{group_id}/stop", post(stop))
        .route("/api/v1/groups/{group_id}/state", post(set_state))
        .route("/api/v1/groups/{group_id}/attach", post(attach))
        .route(
            "/api/v1/groups/{group_id}/scopes/{scope_key}",
            delete(detach),
        )
        .merge(super::group_prompts::routes())
}

async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    let mut response = call(&state, "groups", Map::new()).await?;
    if !principal.is_admin
        && let Some(groups) = response
            .0
            .get_mut("result")
            .and_then(|result| result.get_mut("groups"))
            .and_then(Value::as_array_mut)
    {
        groups.retain(|group| {
            group
                .get("group_id")
                .and_then(Value::as_str)
                .is_some_and(|group_id| principal.allows(group_id))
        });
    }
    Ok(response)
}

async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let mut args = body_object(body)?;
    let op = match args.get("path") {
        None => "group_create",
        Some(Value::String(path)) if !path.trim().is_empty() => {
            args.insert("path".into(), Value::String(path.trim().into()));
            "group_create_with_scope"
        }
        Some(_) => {
            return Err(ApiError::bad_code(
                "invalid_path",
                "path must be a non-empty string",
                json!({}),
            ));
        }
    };
    call(&state, op, args).await
}

async fn show(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(&state, "group_show", object(json!({"group_id":group_id}))).await
}

async fn update(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "group_update", args).await
}

async fn remove(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let mut args = object(
        json!({"group_id":group_id,"by":query.get("by").cloned().unwrap_or_else(||"user".into())}),
    );
    if let Some(confirm) = query.get("confirm") {
        args.insert("confirm".into(), Value::String(confirm.clone()));
    }
    let mut response = call(&state, "group_delete", args).await?;
    cleanup_group_resources(&state, &group_id, &mut response).await;
    Ok(response)
}

#[derive(serde::Deserialize)]
struct ResetQuery {
    #[serde(default)]
    confirm: String,
}

async fn reset(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ResetQuery>,
) -> ApiResult {
    if query.confirm != group_id {
        return Err(ApiError::bad_code(
            "confirm_required",
            format!("confirm must equal group_id: {group_id}"),
            json!({}),
        ));
    }
    let mut response = call(
        &state,
        "group_reset",
        object(json!({"group_id":group_id,"confirm":query.confirm,"by":"user"})),
    )
    .await?;
    cleanup_group_resources(&state, &group_id, &mut response).await;
    Ok(response)
}

async fn cleanup_group_resources(state: &AppState, group_id: &str, response: &mut Json<Value>) {
    state.im_workers.stop(group_id).await;
    let prefixes = [format!("{group_id}::"), format!("web-model::{group_id}::")];
    if let Err(error) = state.browser_surfaces.close_prefixes(&prefixes).await {
        response.0["result"]["browser_cleanup_warning"] = Value::String(error.to_string());
    }
}
async fn start(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "group_start",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}
async fn stop(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    call(
        &state,
        "group_stop",
        object(json!({"group_id":group_id,"by":"user"})),
    )
    .await
}
async fn set_state(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let state_value = query
        .get("state")
        .cloned()
        .ok_or_else(|| ApiError::bad("state is required"))?;
    call(
        &state,
        "group_set_state",
        object(json!({"group_id":group_id,"state":state_value,"by":"user"})),
    )
    .await
}
async fn attach(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "attach", args).await
}
async fn detach(
    State(state): State<AppState>,
    Path((group_id, scope_key)): Path<(String, String)>,
) -> ApiResult {
    call(
        &state,
        "group_detach_scope",
        object(json!({"group_id":group_id,"scope_key":scope_key,"by":"user"})),
    )
    .await
}

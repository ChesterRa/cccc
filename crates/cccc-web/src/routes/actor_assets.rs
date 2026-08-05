use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};

const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/env_private",
            get(secret_keys).post(secret_update),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/avatar",
            get(avatar_get).post(avatar_upload).delete(avatar_clear),
        )
        .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES + 64 * 1024))
}

async fn secret_keys(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    call(
        &state,
        "actor_env_private_keys",
        object(json!({"group_id":group_id,"actor_id":actor_id})),
    )
    .await
}

async fn secret_update(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("actor_id".into(), Value::String(actor_id));
    call(&state, "actor_env_private_update", args).await
}

async fn avatar_get(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> Result<axum::response::Response, ApiError> {
    let response = call(&state, "actor_list", object(json!({"group_id":group_id})))
        .await?
        .0;
    let relative = response
        .get("actors")
        .and_then(Value::as_array)
        .and_then(|actors| {
            actors
                .iter()
                .find(|actor| actor.get("id").and_then(Value::as_str) == Some(&actor_id))
        })
        .and_then(|actor| actor.get("avatar_asset_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let path = cccc_core::blobs::resolve(&state.home, &group_id, relative)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    super::file_response::stream(&path, "application/octet-stream", None, None)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))
}

async fn avatar_upload(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?
    {
        if field.name() == Some("file") {
            data = field
                .bytes()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?
                .to_vec();
            if data.len() > MAX_AVATAR_BYTES {
                return Err(ApiError::bad_code(
                    "avatar_too_large",
                    "avatar file exceeds 2 MiB",
                    json!({"max_bytes":MAX_AVATAR_BYTES}),
                ));
            }
            break;
        }
    }
    if data.is_empty() {
        return Err(ApiError::bad("avatar file is required"));
    }
    let blob = cccc_core::blobs::store(&state.home, &group_id, &data)
        .map_err(|error| ApiError::bad(error.to_string()))?;
    call(&state,"actor_update",object(json!({"group_id":group_id,"actor_id":actor_id,"avatar_asset_path":blob.path,"by":"user"}))).await
}

async fn avatar_clear(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    call(
        &state,
        "actor_update",
        object(json!({"group_id":group_id,"actor_id":actor_id,"avatar_asset_path":"","by":"user"})),
    )
    .await
}

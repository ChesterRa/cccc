use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path, State};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call};
use crate::auth::Principal;

const MAX_REMOTE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/send_cross_group",
            post(send_json),
        )
        .route(
            "/api/v1/groups/{group_id}/send_cross_group_upload",
            post(send_upload),
        )
        .layer(DefaultBodyLimit::max(MAX_REMOTE_UPLOAD_BYTES + 1024 * 1024))
}

async fn send_json(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let destination = super::group_bridge::required(&body, "dst_group_id")?;
    super::group_bridge::ensure_access(&principal, &destination)?;
    if let Some(result) =
        super::group_bridge_session::send_remote(&state, &group_id, &destination, &body).await
    {
        return result;
    }
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(&state, "send_cross_group", args).await
}

async fn send_upload(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut args = serde_json::Map::new();
    let mut files = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?
    {
        let name = field.name().unwrap_or("").to_owned();
        if name == "files" || name == "file" {
            let filename = field.file_name().unwrap_or("attachment").to_owned();
            let content_type = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_owned();
            let data = field
                .bytes()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            if data.len() > MAX_REMOTE_UPLOAD_BYTES {
                return Err(ApiError::bad("remote attachment exceeds 10 MiB"));
            }
            files.push((data, filename, content_type));
        } else {
            let value = field
                .text()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            super::messaging::upload_fields::insert(&mut args, name, value)?;
        }
    }
    let destination = args
        .get("dst_group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad("dst_group_id is required"))?
        .to_owned();
    super::group_bridge::ensure_access(&principal, &destination)?;
    args.insert("dst_group_id".into(), Value::String(destination.clone()));
    args.insert("group_id".into(), Value::String(group_id.clone()));
    let mut preflight_args = args.clone();
    preflight_args.insert("operation".into(), Value::String("send_cross_group".into()));
    preflight_args.insert("has_attachments".into(), Value::Bool(!files.is_empty()));
    let _ = call(&state, "message_upload_preflight", preflight_args).await?;
    let mut attachments = Vec::with_capacity(files.len());
    for (data, filename, content_type) in files {
        let blob = cccc_core::blobs::store(&state.home, &group_id, &data)
            .map_err(|error| ApiError::bad(error.to_string()))?;
        attachments.push(json!({
            "kind":"file","path":blob.path,"title":filename,"mime_type":content_type,
            "bytes":blob.bytes,"sha256":blob.sha256,
            "content_base64":base64::engine::general_purpose::STANDARD.encode(&data)
        }));
    }
    args.insert("attachments".into(), Value::Array(attachments));
    let remote_body = Value::Object(args.clone());
    if let Some(result) = super::group_bridge_session::send_remote(
        &state,
        remote_body["group_id"].as_str().unwrap_or(""),
        &destination,
        &remote_body,
    )
    .await
    {
        return result;
    }
    for attachment in args
        .get_mut("attachments")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(item) = attachment.as_object_mut() {
            item.remove("content_base64");
        }
    }
    call(&state, "send_cross_group", args).await
}

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path, State};
use axum::http::{HeaderValue, Response, header};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use cccc_core::GroupStore;
use cccc_core::group_copy;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/copy/export", get(export))
        .route("/api/v1/groups/copy/preview_import", post(preview))
        .route("/api/v1/groups/copy/import", post(import))
        .route("/api/v1/groups/copy/uploads/{upload_id}", delete(cleanup))
        .layer(DefaultBodyLimit::max(group_copy::MAX_PACKAGE_BYTES))
}

async fn export(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Response<Body>, ApiError> {
    let store = GroupStore::new(state.home).map_err(|error| ApiError::bad(error.to_string()))?;
    let (bytes, _, filename) =
        group_copy::export(&store, &group_id).map_err(|error| ApiError::bad(error.to_string()))?;
    let safe = filename.replace(['\r', '\n', '"'], "_");
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{safe}\"; filename*=UTF-8''{safe}"
        ))
        .map_err(|error| ApiError::bad(error.to_string()))?,
    );
    Ok(response)
}

async fn preview(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    multipart: Multipart,
) -> ApiResult {
    require_admin(&principal)?;
    let upload = read_upload(multipart).await?;
    let bytes = upload
        .data
        .ok_or_else(|| ApiError::bad("file is required"))?;
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    let preview =
        group_copy::preview(&store, &bytes).map_err(|error| ApiError::bad(error.to_string()))?;
    let upload_id = Uuid::new_v4().simple().to_string();
    let path = upload_path(&state, &upload_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ApiError::bad(error.to_string()))?;
    }
    cccc_core::fs::atomic_write(&path, &bytes).map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(Json(
        json!({"ok":true,"result":{"preview":preview,"upload_id":upload_id}}),
    ))
}

async fn import(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    multipart: Multipart,
) -> ApiResult {
    require_admin(&principal)?;
    let upload = read_upload(multipart).await?;
    let upload_id = field(&upload.fields, "upload_id");
    let staged = if upload_id.is_empty() {
        None
    } else {
        Some(upload_path(&state, &upload_id)?)
    };
    let bytes = if let Some(data) = upload.data {
        data
    } else if let Some(path) = &staged {
        fs::read(path).map_err(|_| ApiError::not_found("group copy upload not found"))?
    } else {
        return Err(ApiError::bad("file or upload_id is required"));
    };
    let store = GroupStore::new(state.home).map_err(|error| ApiError::bad(error.to_string()))?;
    let result = group_copy::import(
        &store,
        &bytes,
        &field(&upload.fields, "workspace_root"),
        &field(&upload.fields, "title"),
    )
    .map_err(|error| ApiError::bad(error.to_string()))?;
    if let Some(path) = staged {
        let _ = fs::remove_file(path);
    }
    Ok(Json(json!({"ok":true,"result":result})))
}

async fn cleanup(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(upload_id): Path<String>,
) -> ApiResult {
    require_admin(&principal)?;
    let path = upload_path(&state, &upload_id)?;
    let deleted = match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(ApiError::bad(error.to_string())),
    };
    Ok(Json(
        json!({"ok":true,"result":{"upload_id":upload_id,"deleted":deleted}}),
    ))
}

struct Upload {
    fields: Map<String, Value>,
    data: Option<Vec<u8>>,
}

async fn read_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let mut upload = Upload {
        fields: Map::new(),
        data: None,
    };
    while let Some(mut field_data) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?
    {
        let name = field_data.name().unwrap_or("").to_owned();
        if name == "file" {
            let mut data = Vec::new();
            while let Some(chunk) = field_data
                .chunk()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?
            {
                checked_extend(&mut data, &chunk)?;
            }
            upload.data = Some(data);
        } else {
            upload.fields.insert(
                name,
                Value::String(
                    field_data
                        .text()
                        .await
                        .map_err(|error| ApiError::bad(error.to_string()))?,
                ),
            );
        }
    }
    Ok(upload)
}

fn checked_extend(data: &mut Vec<u8>, chunk: &Bytes) -> Result<(), ApiError> {
    if data.len().saturating_add(chunk.len()) > group_copy::MAX_PACKAGE_BYTES {
        return Err(ApiError::bad("group copy package exceeds 1 GiB"));
    }
    data.extend_from_slice(chunk);
    Ok(())
}

fn upload_path(state: &AppState, upload_id: &str) -> Result<PathBuf, ApiError> {
    if upload_id.len() != 32 || !upload_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::bad("invalid upload_id"));
    }
    Ok(state
        .home
        .root()
        .join("tmp/group-copy-uploads")
        .join(format!("{upload_id}.zip")))
}

fn field(fields: &Map<String, Value>, name: &str) -> String {
    fields
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .into()
}

fn require_admin(principal: &Principal) -> Result<(), ApiError> {
    if principal.is_admin {
        Ok(())
    } else {
        Err(ApiError::forbidden("administrator access required"))
    }
}

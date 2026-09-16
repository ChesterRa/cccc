//! Scoped file management. Uploads publish only after receiving the complete body.
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
    auth::Principal,
    connect_frames::live_group_access,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, Request, State},
};
use cccc_core::workspace::{self, WorkspaceUpload};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(super) enum Operation {
    Create { path: String, directory: bool },
    Move { path: String, destination: String },
    Delete { path: String },
}
#[derive(Deserialize)]
pub(super) struct EntryRequest {
    scope_key: String,
    scope_url: String,
    #[serde(flatten)]
    operation: Operation,
}

pub(super) async fn change(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<EntryRequest>,
) -> ApiResult {
    tokio::task::spawn_blocking(move || {
        let group = super::load_group(&state, &group_id, &body.scope_key, &body.scope_url)?;
        let result = match body.operation {
            Operation::Create { path, directory } => {
                let path = workspace::create_entry(&group, &path, directory)
                    .map_err(|e| super::path_error(&path, e))?;
                json!({"path": path})
            }
            Operation::Move { path, destination } => {
                let (path, destination) = workspace::move_entry(&group, &path, &destination)
                    .map_err(|e| super::path_error(&path, e))?;
                let mime_type = mime_guess::from_path(&destination)
                    .first_or_octet_stream()
                    .to_string();
                json!({"path": path, "destination": destination, "mime_type": mime_type})
            }
            Operation::Delete { path } => {
                let path = workspace::delete_entry(&group, &path)
                    .map_err(|e| super::path_error(&path, e))?;
                json!({"path": path})
            }
        };
        Ok(success(result))
    })
    .await
    .map_err(|e| ApiError::unavailable("workspace_unavailable", e.to_string()))?
}

#[derive(Deserialize)]
pub(super) struct UploadQuery {
    scope_key: String,
    scope_url: String,
    path: String,
    bytes: u64,
    connect_frame: Option<String>,
}
const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

pub(super) async fn upload(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<UploadQuery>,
    request: Request,
) -> ApiResult {
    if query.bytes > MAX_UPLOAD_BYTES {
        return Err(ApiError::bad_code(
            "upload_too_large",
            "Uploads are limited to 100 MiB per file",
            json!({}),
        ));
    }
    let state_start = state.clone();
    let group_start = group_id.clone();
    let (upload, query) = tokio::task::spawn_blocking(move || {
        let group = super::load_group(
            &state_start,
            &group_start,
            &query.scope_key,
            &query.scope_url,
        )?;
        let upload = WorkspaceUpload::new(&group, &query.path)
            .map_err(|e| super::path_error(&query.path, e))?;
        Ok::<_, ApiError>((upload, query))
    })
    .await
    .map_err(|e| ApiError::unavailable("workspace_unavailable", e.to_string()))??;
    let writer = upload
        .writer()
        .map_err(|e| super::path_error(&query.path, e))?;
    let mut writer = tokio::fs::File::from_std(writer);
    let mut stream = request.into_body().into_data_stream();
    let mut received = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ApiError::bad(e.to_string()))?;
        received = received.saturating_add(chunk.len() as u64);
        if received > query.bytes {
            return Err(ApiError::bad_code(
                "upload_size_mismatch",
                "The uploaded file size does not match the selection",
                json!({}),
            ));
        }
        writer
            .write_all(&chunk)
            .await
            .map_err(|e| super::path_error(&query.path, e))?;
    }
    writer
        .flush()
        .await
        .map_err(|e| super::path_error(&query.path, e))?;
    drop(writer);
    if received != query.bytes {
        return Err(ApiError::bad_code(
            "upload_size_mismatch",
            "The upload was incomplete; no file was created",
            json!({}),
        ));
    }
    tokio::task::spawn_blocking(move || {
        // A large upload may outlive its token, embedded frame or active workspace.
        if !live_group_access(
            &state,
            &principal,
            &group_id,
            query.connect_frame.as_deref(),
        ) {
            return Err(ApiError::forbidden_code(
                "access_revoked",
                "Workspace access has ended",
            ));
        }
        let group = super::load_group(&state, &group_id, &query.scope_key, &query.scope_url)?;
        let path = upload
            .finish(&group)
            .map_err(|e| super::path_error(&query.path, e))?;
        Ok(success(json!({"path": path, "bytes": received})))
    })
    .await
    .map_err(|e| ApiError::unavailable("workspace_unavailable", e.to_string()))?
}

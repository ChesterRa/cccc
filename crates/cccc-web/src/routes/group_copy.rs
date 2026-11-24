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
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, format_byte_limit};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/copy/export", get(export))
        .route("/api/v1/groups/copy/preview_import", post(preview))
        .route("/api/v1/groups/copy/import", post(import))
        .route("/api/v1/groups/copy/uploads/{upload_id}", delete(cleanup))
        .layer(DefaultBodyLimit::max(
            group_copy::MAX_PACKAGE_BYTES + 1024 * 1024,
        ))
}

async fn export(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
) -> Result<Response<Body>, ApiError> {
    let permit = acquire_copy_permit().await?;
    let home = state.home;
    let (bytes, _, filename) = run_copy_task(permit, move || {
        let store = GroupStore::new(home)?;
        group_copy::export(&store, &group_id)
    })
    .await?;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&super::file_response::content_disposition(
            "attachment",
            &filename,
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
    let permit = acquire_copy_permit().await?;
    let home = state.home.clone();
    let (preview, bytes) = run_copy_task(permit, move || {
        let store = GroupStore::new(home)?;
        let preview = group_copy::preview(&store, &bytes)?;
        Ok((preview, bytes))
    })
    .await?;
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
        tokio::fs::read(path)
            .await
            .map_err(|_| ApiError::not_found("group copy upload not found"))?
    } else {
        return Err(ApiError::bad("file or upload_id is required"));
    };
    let permit = acquire_copy_permit().await?;
    let workspace_root = field(&upload.fields, "workspace_root");
    let title = field(&upload.fields, "title");
    let home = state.home;
    let result = run_copy_task(permit, move || {
        let store = GroupStore::new(home)?;
        group_copy::import(&store, &bytes, &workspace_root, &title)
    })
    .await?;
    let cleanup_warning = staged.and_then(cleanup_staged_upload);
    let mut result =
        serde_json::to_value(result).map_err(|error| ApiError::bad(error.to_string()))?;
    result["cleanup_warning"] = cleanup_warning.unwrap_or(Value::Null);
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

fn package_too_large() -> ApiError {
    ApiError::payload_too_large(
        "group_copy_too_large",
        format!(
            "group copy package exceeds {}",
            format_byte_limit(group_copy::MAX_PACKAGE_BYTES)
        ),
    )
    .with_details(json!({"max_bytes": group_copy::MAX_PACKAGE_BYTES}))
}

async fn read_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let mut upload = Upload {
        fields: Map::new(),
        data: None,
    };
    while let Some(mut field_data) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::from_multipart(error, package_too_large))?
    {
        let name = field_data.name().unwrap_or("").to_owned();
        if name == "file" {
            let mut data = Vec::new();
            while let Some(chunk) = field_data
                .chunk()
                .await
                .map_err(|error| ApiError::from_multipart(error, package_too_large))?
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
                        .map_err(|error| ApiError::from_multipart(error, package_too_large))?,
                ),
            );
        }
    }
    Ok(upload)
}

fn checked_extend(data: &mut Vec<u8>, chunk: &Bytes) -> Result<(), ApiError> {
    if data.len().saturating_add(chunk.len()) > group_copy::MAX_PACKAGE_BYTES {
        return Err(package_too_large());
    }
    data.extend_from_slice(chunk);
    Ok(())
}

fn copy_semaphore() -> &'static Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(1)))
}

async fn acquire_copy_permit() -> Result<OwnedSemaphorePermit, ApiError> {
    copy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| ApiError::unavailable("group_copy_unavailable", error.to_string()))
}

async fn run_copy_task<T: Send + 'static>(
    permit: OwnedSemaphorePermit,
    task: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| ApiError::unavailable("group_copy_task_failed", error.to_string()))?
    .map_err(|error| ApiError::bad(error.to_string()))
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

fn cleanup_staged_upload(path: PathBuf) -> Option<Value> {
    match fs::remove_file(&path) {
        Ok(()) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => Some(json!({
            "code":"group_copy_cleanup_failed",
            "message":format!("group import committed but staged upload cleanup failed: {error}"),
            "upload_path":path,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn blocking_task_panic_releases_the_copy_permit() {
        let permit = acquire_copy_permit().await.expect("first permit");
        let result: Result<(), ApiError> = run_copy_task(permit, || {
            panic!("simulated group-copy worker failure");
        })
        .await;
        assert!(result.is_err());

        let permit = tokio::time::timeout(Duration::from_secs(1), acquire_copy_permit())
            .await
            .expect("permit was leaked")
            .expect("second permit");
        drop(permit);
    }

    #[test]
    fn committed_import_cleanup_failure_is_returned_as_a_warning() {
        let directory = tempfile::tempdir().expect("tempdir");
        let warning = cleanup_staged_upload(directory.path().to_path_buf()).expect("warning");
        assert_eq!(warning["code"], "group_copy_cleanup_failed");
        assert!(directory.path().is_dir());
    }

    #[tokio::test]
    async fn multipart_body_limit_maps_to_group_copy_too_large() {
        use axum::http::{Request, StatusCode};
        use cccc_core::HomeLayout;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let principal = Principal {
            user_id: "admin".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: String::new(),
        };
        let app = Router::new()
            .route("/api/v1/groups/copy/preview_import", post(preview))
            .route("/api/v1/groups/copy/import", post(import))
            .layer(DefaultBodyLimit::max(64))
            .layer(Extension(principal))
            .with_state(crate::test_state(home));

        let payload = format!(
            "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"copy.zip\"\r\nContent-Type: application/zip\r\n\r\n{}\r\n--b--\r\n",
            "x".repeat(256)
        );
        for uri in [
            "/api/v1/groups/copy/preview_import",
            "/api/v1/groups/copy/import",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(uri)
                        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=b")
                        .body(Body::from(payload.clone()))
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
            let body = response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            let json: Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(
                json["error"]["code"], "group_copy_too_large",
                "{uri}: {json}"
            );
            assert_eq!(json["error"]["message"], "group copy package exceeds 1 GiB");
            assert_eq!(
                json["error"]["details"]["max_bytes"],
                group_copy::MAX_PACKAGE_BYTES
            );
        }
    }
}

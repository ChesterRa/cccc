use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};
use crate::auth::Principal;

const MAX_LOCAL_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MULTIPART_OVERHEAD_BYTES: usize = 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/send", post(send))
        .route("/api/v1/groups/{group_id}/tracked_send", post(tracked_send))
        .route(
            "/api/v1/groups/{group_id}/delegate_contact",
            post(delegate_contact),
        )
        .route(
            "/api/v1/groups/{group_id}/slash_skill_dispatch",
            post(slash_skill_dispatch),
        )
        .route("/api/v1/groups/{group_id}/reply", post(reply))
        .route("/api/v1/groups/{group_id}/events/{event_id}/ack", post(ack))
        .route(
            "/api/v1/groups/{group_id}/inbox/{actor_id}",
            get(inbox_list),
        )
        .route(
            "/api/v1/groups/{group_id}/inbox/{actor_id}/read",
            post(inbox_read),
        )
        .route(
            "/api/v1/groups/{group_id}/blobs/{blob_name}",
            get(blob_download),
        )
        .merge(upload_routes())
}

fn upload_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/send_upload", post(send_upload))
        .route("/api/v1/groups/{group_id}/reply_upload", post(reply_upload))
        .layer(DefaultBodyLimit::max(
            MAX_LOCAL_UPLOAD_BYTES + MULTIPART_OVERHEAD_BYTES,
        ))
}

async fn send(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "send", group_id, body).await
}
async fn tracked_send(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "tracked_send", group_id, body).await
}
async fn delegate_contact(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let destination = super::group_bridge::required(&body, "dst_group_id")?;
    super::group_bridge::ensure_access(&principal, &destination)?;
    daemon_body(&state, "relay_user_delegation", group_id, body).await
}
async fn slash_skill_dispatch(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "slash_skill_dispatch", group_id, body).await
}
async fn reply(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "reply", group_id, body).await
}
async fn ack(
    State(state): State<AppState>,
    Path((group_id, event_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let _ = body_object(body)?;
    let actor_id = "user";
    call(
        &state,
        "chat_ack",
        object(json!({"group_id":group_id,"event_id":event_id,"actor_id":actor_id,"by":actor_id})),
    )
    .await
}
async fn inbox_list(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Query(query): Query<InboxQuery>,
) -> ApiResult {
    call(
        &state,
        "inbox_list",
        object(json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "by":"user",
            "limit":query.limit.unwrap_or(50).clamp(1, 1000),
        })),
    )
    .await
}

#[derive(serde::Deserialize)]
struct InboxQuery {
    limit: Option<u64>,
}
async fn inbox_read(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("actor_id".into(), Value::String(actor_id));
    call(&state, "inbox_mark_read", args).await
}

async fn send_upload(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    upload(&state, &group_id, multipart, false).await
}
async fn reply_upload(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    upload(&state, &group_id, multipart, true).await
}
async fn upload(
    state: &AppState,
    group_id: &str,
    mut multipart: Multipart,
    is_reply: bool,
) -> ApiResult {
    let mut args = serde_json::Map::new();
    let mut attachments = Vec::new();
    let mut staged_uploads = Vec::new();
    let mut uploaded_bytes = 0_usize;
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
            let mut upload = cccc_core::blobs::BlobUpload::new(&state.home, group_id)
                .map_err(|error| ApiError::bad(error.to_string()))?;
            let mut field = field;
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?
            {
                uploaded_bytes = uploaded_bytes.saturating_add(chunk.len());
                if uploaded_bytes > MAX_LOCAL_UPLOAD_BYTES {
                    return Err(ApiError::bad("attachments exceed 100 MiB in total"));
                }
                upload
                    .write_chunk(&chunk)
                    .map_err(|error| ApiError::bad(error.to_string()))?;
            }
            staged_uploads.push((upload, filename, content_type));
        } else {
            let value = field
                .text()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            insert_upload_field(&mut args, name, value)?;
        }
    }
    for (upload, filename, content_type) in staged_uploads {
        let blob = upload
            .finish()
            .map_err(|error| ApiError::bad(error.to_string()))?;
        attachments.push(json!({"kind":"file","path":blob.path,"title":filename,"mime_type":content_type,"bytes":blob.bytes,"sha256":blob.sha256}));
    }
    args.insert("group_id".into(), Value::String(group_id.into()));
    args.insert("attachments".into(), Value::Array(attachments));
    call(state, if is_reply { "reply" } else { "send" }, args).await
}

pub(super) fn insert_upload_field(
    args: &mut serde_json::Map<String, Value>,
    name: String,
    value: String,
) -> Result<(), ApiError> {
    match name.as_str() {
        "to_json" => {
            args.insert(
                "to".into(),
                serde_json::from_str(&value).unwrap_or_else(|_| json!([])),
            );
        }
        "refs_json" => {
            let refs = serde_json::from_str::<Value>(&value).map_err(|error| {
                ApiError::bad_code("invalid_refs", error.to_string(), json!({}))
            })?;
            let refs = refs.as_array().ok_or_else(|| {
                ApiError::bad_code("invalid_refs", "refs_json must be a JSON array", json!({}))
            })?;
            args.insert(
                "refs".into(),
                Value::Array(
                    refs.iter()
                        .filter(|item| item.is_object())
                        .cloned()
                        .collect(),
                ),
            );
        }
        "reply_required" => {
            args.insert(
                name,
                Value::Bool(matches!(value.as_str(), "true" | "1" | "yes")),
            );
        }
        _ => {
            args.insert(name, Value::String(value));
        }
    }
    Ok(())
}

async fn blob_download(
    State(state): State<AppState>,
    Path((group_id, blob_name)): Path<(String, String)>,
) -> Result<axum::response::Response, ApiError> {
    let path = cccc_core::blobs::resolve(&state.home, &group_id, &blob_name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let prefix = super::file_response::prefix(&path, 16)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let content_type = blob_content_type(&blob_name, &prefix);
    super::file_response::stream(&path, &content_type, None, None)
        .await
        .map_err(|error| ApiError::not_found(error.to_string()))
}

fn blob_content_type(blob_name: &str, bytes: &[u8]) -> String {
    let guessed = mime_guess::from_path(blob_name).first_or_octet_stream();
    if guessed != mime_guess::mime::APPLICATION_OCTET_STREAM {
        return guessed.essence_str().to_owned();
    }
    let detected = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        "image/avif"
    } else {
        "application/octet-stream"
    };
    detected.to_owned()
}

async fn daemon_body(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(state, op, args).await
}

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};

const MAX_LOCAL_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MULTIPART_OVERHEAD_BYTES: usize = 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/send", post(send))
        .route(
            "/api/v1/groups/{group_id}/send_cross_group",
            post(send_cross_group),
        )
        .route("/api/v1/groups/{group_id}/tracked_send", post(tracked_send))
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
        .route(
            "/api/v1/groups/{group_id}/send_cross_group_upload",
            post(send_cross_group_upload),
        )
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
async fn send_cross_group(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let destination = body["dst_group_id"].as_str().unwrap_or("").to_owned();
    if let Some(result) =
        super::group_bridge_session::send_remote(&state, &group_id, &destination, &body).await
    {
        return result;
    }
    daemon_body(&state, "send_cross_group", group_id, body).await
}
async fn tracked_send(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, "tracked_send", group_id, body).await
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
    let actor_id = body
        .get("actor_id")
        .or_else(|| body.get("by"))
        .and_then(Value::as_str)
        .unwrap_or("user");
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
) -> ApiResult {
    call(
        &state,
        "inbox_list",
        object(json!({"group_id":group_id,"actor_id":actor_id,"by":"user","limit":1000})),
    )
    .await
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
async fn send_cross_group_upload(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut args = serde_json::Map::new();
    let mut attachments = Vec::new();
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
            if data.len() > 10 * 1024 * 1024 {
                return Err(ApiError::bad("remote attachment exceeds 10 MiB"));
            }
            let blob = cccc_core::blobs::store(&state.home, &group_id, &data)
                .map_err(|error| ApiError::bad(error.to_string()))?;
            attachments.push(json!({
                "kind":"file","path":blob.path,"title":filename,"mime_type":content_type,
                "bytes":blob.bytes,"sha256":blob.sha256,
                "content_base64":base64::engine::general_purpose::STANDARD.encode(&data)
            }));
        } else {
            let value = field
                .text()
                .await
                .map_err(|error| ApiError::bad(error.to_string()))?;
            if name == "to_json" {
                args.insert(
                    "to".into(),
                    serde_json::from_str(&value).unwrap_or_else(|_| json!([])),
                );
            } else if name == "reply_required" {
                args.insert(
                    name,
                    Value::Bool(matches!(value.as_str(), "true" | "1" | "yes")),
                );
            } else {
                args.insert(name, Value::String(value));
            }
        }
    }
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("attachments".into(), Value::Array(attachments));
    let destination = args["dst_group_id"].as_str().unwrap_or("").to_owned();
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
    if let Some(attachments) = args.get_mut("attachments").and_then(Value::as_array_mut) {
        for attachment in attachments {
            if let Some(item) = attachment.as_object_mut() {
                item.remove("content_base64");
            }
        }
    }
    call(&state, "send_cross_group", args).await
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
            insert_upload_field(&mut args, name, value);
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

fn insert_upload_field(args: &mut serde_json::Map<String, Value>, name: String, value: String) {
    match name.as_str() {
        "to_json" => {
            args.insert(
                "to".into(),
                serde_json::from_str(&value).unwrap_or_else(|_| json!([])),
            );
        }
        "refs_json" => {
            args.insert(
                "refs".into(),
                serde_json::from_str(&value).unwrap_or_else(|_| json!([])),
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
}

async fn blob_download(
    State(state): State<AppState>,
    Path((group_id, blob_name)): Path<(String, String)>,
) -> Result<axum::response::Response, ApiError> {
    let path = cccc_core::blobs::resolve(&state.home, &group_id, &blob_name)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    let bytes = std::fs::read(path).map_err(|error| ApiError::not_found(error.to_string()))?;
    let content_type = blob_content_type(&blob_name, &bytes);
    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response())
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

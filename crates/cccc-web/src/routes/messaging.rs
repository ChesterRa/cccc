use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};
use crate::auth::Principal;

mod oversized_text;
pub(super) mod upload_fields;

const MAX_LOCAL_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MAX_MESSAGE_JSON_BYTES: usize = 12 * 1024 * 1024;
const MULTIPART_OVERHEAD_BYTES: usize = 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(json_routes())
        .route(
            "/api/v1/groups/{group_id}/messages/{source_event_id}/deliver",
            post(message_deliver),
        )
        .route(
            "/api/v1/groups/{group_id}/messages/{source_event_id}/reply-request/cancel",
            post(reply_request_cancel),
        )
        .route(
            "/api/v1/groups/{group_id}/inbox/{actor_id}",
            get(inbox_peek),
        )
        .route(
            "/api/v1/groups/{group_id}/inbox/{actor_id}/read",
            post(inbox_read),
        )
        .route(
            "/api/v1/groups/{group_id}/blobs/{blob_name}",
            get(super::blob_download::download),
        )
        .merge(upload_routes())
}

fn json_routes() -> Router<AppState> {
    Router::new()
        .merge(large_message_json_routes())
        .route("/api/v1/groups/{group_id}/tracked_send", post(tracked_send))
        .route(
            "/api/v1/groups/{group_id}/delegate_contact",
            post(delegate_contact),
        )
        .route(
            "/api/v1/groups/{group_id}/slash_skill_dispatch",
            post(slash_skill_dispatch),
        )
}

fn large_message_json_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/send", post(send))
        .route("/api/v1/groups/{group_id}/reply", post(reply))
        .layer(DefaultBodyLimit::max(MAX_MESSAGE_JSON_BYTES))
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
    let mut args = body_object(body)?;
    oversized_text::dispatch(&state, group_id, "send", &mut args).await
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
    let mut args = body_object(body)?;
    args.remove("quote_text");
    oversized_text::dispatch(&state, group_id, "reply", &mut args).await
}
async fn message_deliver(
    State(state): State<AppState>,
    Path((group_id, source_event_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("source_event_id".into(), Value::String(source_event_id));
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "message_deliver", args).await
}
async fn reply_request_cancel(
    State(state): State<AppState>,
    Path((group_id, source_event_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    if !args.is_empty() {
        return Err(ApiError::bad(
            "reply-request cancellation body must be empty",
        ));
    }
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("source_event_id".into(), Value::String(source_event_id));
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "reply_request_cancel", args).await
}
async fn inbox_peek(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Query(query): Query<InboxQuery>,
) -> ApiResult {
    call(
        &state,
        "inbox_peek",
        object(json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "by":"user",
            "limit":query.limit.unwrap_or(50),
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
    args.insert("by".into(), Value::String("user".into()));
    call(&state, "inbox_read", args).await
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
            upload_fields::insert(&mut args, name, value)?;
        }
    }
    if is_reply {
        let message_mode = args
            .get("message_mode")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("send")
            .to_owned();
        if !matches!(message_mode.as_str(), "send" | "mail") {
            return Err(ApiError::bad_code(
                "invalid_message_mode",
                "reply message_mode must be send or mail",
                json!({}),
            ));
        }
        args.insert("message_mode".into(), Value::String(message_mode));
    }
    args.insert("group_id".into(), Value::String(group_id.into()));
    let mut preflight_args = args.clone();
    preflight_args.insert(
        "operation".into(),
        Value::String(if is_reply { "reply" } else { "send" }.into()),
    );
    preflight_args.insert(
        "has_attachments".into(),
        Value::Bool(!staged_uploads.is_empty()),
    );
    let Json(preflight_response) = call(state, "message_upload_preflight", preflight_args).await?;
    let preflight = preflight_response
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if preflight.get("duplicate").and_then(Value::as_bool) == Some(true) {
        let result = preflight
            .get("result")
            .cloned()
            .unwrap_or_else(|| json!({}));
        return Ok(Json(json!({"ok":true,"result":result})));
    }
    for (upload, filename, content_type) in staged_uploads {
        let blob = upload
            .finish()
            .map_err(|error| ApiError::bad(error.to_string()))?;
        attachments.push(json!({"kind":"file","path":blob.path,"title":filename,"mime_type":content_type,"bytes":blob.bytes,"sha256":blob.sha256}));
    }
    args.insert("attachments".into(), Value::Array(attachments));
    call(state, if is_reply { "reply" } else { "send" }, args).await
}

async fn daemon_body(state: &AppState, op: &str, group_id: String, body: Value) -> ApiResult {
    daemon_args(state, op, group_id, body_object(body)?).await
}

async fn daemon_args(
    state: &AppState,
    op: &str,
    group_id: String,
    mut args: serde_json::Map<String, Value>,
) -> ApiResult {
    args.insert("group_id".into(), Value::String(group_id));
    call(state, op, args).await
}

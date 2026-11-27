use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, format_byte_limit, object};
use crate::auth::Principal;

mod oversized_text;
pub(super) mod upload_fields;
pub(super) mod web_authority;

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
            "/api/v1/groups/{group_id}/messages/{source_event_id}/reply-request/answer",
            post(reply_request_answer),
        )
        .route(
            "/api/v1/groups/{group_id}/reply-requests/pending",
            get(user_reply_requests),
        )
        .route(
            "/api/v1/reply-requests/summary",
            get(user_reply_requests_summary),
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
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    web_authority::prepare_user_message(&mut args, &principal)?;
    oversized_text::dispatch(&state, group_id, "send", &mut args).await
}
async fn tracked_send(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, &principal, "tracked_send", group_id, body).await
}
async fn slash_skill_dispatch(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    daemon_body(&state, &principal, "slash_skill_dispatch", group_id, body).await
}
async fn reply(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    web_authority::prepare_user_message(&mut args, &principal)?;
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
async fn reply_request_answer(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, source_event_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    if let Some(key) = args
        .keys()
        .find(|key| !matches!(key.as_str(), "client_id" | "answer"))
    {
        return Err(ApiError::bad(format!(
            "reply-request answer accepts client_id and answer only, not {key}"
        )));
    }
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("source_event_id".into(), Value::String(source_event_id));
    args.insert("by".into(), Value::String("user".into()));
    args.insert(
        cccc_core::actors::CHET_WEB_ADMISSION_ARG.into(),
        Value::Bool(principal.is_admin),
    );
    call(&state, "reply_request_answer", args).await
}
#[derive(serde::Deserialize)]
struct PendingQuery {
    before: Option<String>,
    limit: Option<u64>,
}
async fn user_reply_requests(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<PendingQuery>,
) -> ApiResult {
    let mut args = object(json!({"group_id": group_id, "by": "user"}));
    if let Some(before) = query.before {
        args.insert("before".into(), Value::String(before));
    }
    if let Some(limit) = query.limit {
        args.insert("limit".into(), json!(limit));
    }
    call(&state, "user_reply_requests", args).await
}
/// Only Groups this principal may open are counted; a non-admin never learns
/// another Group's id or count. Administrators also receive a read-only
/// `unattended` breakdown folded against live in-memory Chet status, while
/// internal `chet_breakdown` (including Chet UUIDs) is always stripped.
async fn user_reply_requests_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    let mut args = object(json!({"by": "user"}));
    if principal.is_admin {
        args.insert("include_chet_breakdown".into(), Value::Bool(true));
    } else {
        args.insert("group_ids".into(), json!(principal.allowed_groups));
    }
    let Json(mut payload) = call(&state, "user_reply_requests_summary", args).await?;
    let needs_live_status = payload
        .get("result")
        .and_then(|result| result.get("groups"))
        .and_then(Value::as_array)
        .is_some_and(|groups| summary_needs_live_chet_status(groups));
    let live_status = if principal.is_admin && needs_live_status {
        Some(state.codex_voice.chet_status().await)
    } else {
        None
    };
    if let Some(groups) = payload
        .get_mut("result")
        .and_then(|result| result.get_mut("groups"))
        .and_then(Value::as_array_mut)
    {
        fold_summary_chet_breakdowns(
            groups,
            principal.is_admin,
            live_status.as_ref(),
            state.codex_voice.chet_group_input_active(),
        );
    }
    Ok(Json(payload))
}

fn summary_needs_live_chet_status(groups: &[Value]) -> bool {
    groups.iter().any(|group| {
        let Some(bd) = group.get("chet_breakdown") else {
            return false;
        };
        if bd.get("state").and_then(Value::as_str) != Some("fresh") {
            return false;
        }
        let waiting = bd.get("waiting").and_then(Value::as_u64).unwrap_or(0);
        let waiting_unregistered = bd
            .get("waiting_unregistered")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let has_dispatched = bd
            .get("dispatched_input_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| !ids.is_empty());
        waiting > 0 || waiting_unregistered > 0 || has_dispatched
    })
}

fn fold_summary_chet_breakdowns(
    groups: &mut [Value],
    is_admin: bool,
    live_status: Option<&Value>,
    group_input_active: bool,
) {
    let string_set = |key: &str| -> std::collections::HashSet<&str> {
        live_status
            .and_then(|status| status.get(key))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    };
    let preparing = string_set("preparing");
    let queued = string_set("queued");
    let working = string_set("working");
    let owner_busy = live_status
        .and_then(|status| status.get("owner_busy"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let history_write_failed = live_status
        .and_then(|status| status.get("history_write_failed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let has_start_problem = live_status
        .and_then(|status| status.get("start_problem"))
        .is_some_and(|problem| !problem.is_null());

    for group in groups {
        let Some(object) = group.as_object_mut() else {
            continue;
        };
        // Always strip internal `chet_breakdown` regardless of admin status or pending counts.
        let Some(breakdown) = object.remove("chet_breakdown") else {
            continue;
        };
        if !is_admin {
            continue;
        }
        let unknown_unattended = || {
            json!({
                "state": "unknown",
                "needs_user": null,
                "with_chet": null,
                "unknown": null,
            })
        };
        let unattended = if breakdown.get("state").and_then(Value::as_str) != Some("fresh") {
            unknown_unattended()
        } else if let (
            Some(pending),
            Some(needs_user),
            Some(waiting),
            Some(waiting_unregistered),
            Some(dispatched),
        ) = (
            object.get("pending").and_then(Value::as_u64),
            breakdown.get("needs_user").and_then(Value::as_u64),
            breakdown.get("waiting").and_then(Value::as_u64),
            breakdown
                .get("waiting_unregistered")
                .and_then(Value::as_u64),
            breakdown
                .get("dispatched_input_ids")
                .and_then(Value::as_array),
        ) {
            let mut with_chet = 0_u64;
            let mut unknown = 0_u64;

            if has_start_problem {
                unknown = unknown.saturating_add(waiting);
            } else {
                with_chet = with_chet.saturating_add(waiting);
            }
            if has_start_problem || history_write_failed || !group_input_active {
                unknown = unknown.saturating_add(waiting_unregistered);
            } else {
                with_chet = with_chet.saturating_add(waiting_unregistered);
            }
            for id in dispatched {
                let Some(id_str) = id.as_str() else {
                    unknown = unknown.saturating_add(1);
                    continue;
                };
                if preparing.contains(id_str) {
                    with_chet = with_chet.saturating_add(1);
                } else if owner_busy {
                    unknown = unknown.saturating_add(1);
                } else if queued.contains(id_str) || working.contains(id_str) {
                    with_chet = with_chet.saturating_add(1);
                } else {
                    unknown = unknown.saturating_add(1);
                }
            }
            if needs_user
                .checked_add(with_chet)
                .and_then(|sum| sum.checked_add(unknown))
                == Some(pending)
            {
                json!({
                    "state": "fresh",
                    "needs_user": needs_user,
                    "with_chet": with_chet,
                    "unknown": unknown,
                })
            } else {
                unknown_unattended()
            }
        } else {
            unknown_unattended()
        };
        object.insert("unattended".into(), unattended);
    }
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
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    upload(&state, &principal, &group_id, multipart, false).await
}
async fn reply_upload(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    multipart: Multipart,
) -> ApiResult {
    upload(&state, &principal, &group_id, multipart, true).await
}
fn attachment_too_large() -> ApiError {
    ApiError::bad_code(
        "attachment_too_large",
        format!(
            "attachments exceed {} in total",
            format_byte_limit(MAX_LOCAL_UPLOAD_BYTES)
        ),
        json!({}),
    )
}

async fn upload(
    state: &AppState,
    principal: &Principal,
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
        .map_err(|error| ApiError::from_multipart(error, attachment_too_large))?
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
                .map_err(|error| ApiError::from_multipart(error, attachment_too_large))?
            {
                uploaded_bytes = uploaded_bytes.saturating_add(chunk.len());
                if uploaded_bytes > MAX_LOCAL_UPLOAD_BYTES {
                    return Err(attachment_too_large());
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
                .map_err(|error| ApiError::from_multipart(error, attachment_too_large))?;
            upload_fields::insert(&mut args, name, value)?;
        }
    }
    web_authority::prepare_user_message(&mut args, principal)?;
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
    // A send naming another Group crosses Groups; the daemon copies the files.
    let destination = args
        .get("dst_group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let operation = match (&destination, is_reply) {
        (_, true) => "reply",
        (Some(destination), false) => {
            // Same rule as the JSON cross-group route, before any file is kept.
            super::messaging_cross_group::ensure_access(principal, destination)?;
            "send_cross_group"
        }
        (None, false) => "send",
    };
    let mut preflight_args = args.clone();
    preflight_args.insert("operation".into(), Value::String(operation.into()));
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
    call(state, operation, args).await
}

async fn daemon_body(
    state: &AppState,
    principal: &Principal,
    op: &str,
    group_id: String,
    body: Value,
) -> ApiResult {
    let mut args = body_object(body)?;
    web_authority::prepare_user_message(&mut args, principal)?;
    daemon_args(state, op, group_id, args).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use cccc_core::{GroupStore, HomeLayout};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn multipart_body_limit_maps_to_attachment_too_large() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("messaging limit", "")
            .expect("group");
        let principal = Principal {
            user_id: "user".into(),
            allowed_groups: vec![group.group_id.clone()],
            is_admin: true,
            raw_token: String::new(),
        };
        let app = Router::new()
            .route("/api/v1/groups/{group_id}/send_upload", post(send_upload))
            .route("/api/v1/groups/{group_id}/reply_upload", post(reply_upload))
            .layer(DefaultBodyLimit::max(64))
            .layer(Extension(principal))
            .with_state(crate::test_state(home));

        let payload = format!(
            "--b\r\nContent-Disposition: form-data; name=\"files\"; filename=\"big.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n{}\r\n--b--\r\n",
            "x".repeat(256)
        );
        for endpoint in ["send_upload", "reply_upload"] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/api/v1/groups/{}/{endpoint}", group.group_id))
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
                json["error"]["code"], "attachment_too_large",
                "{endpoint}: {json}"
            );
            assert_eq!(
                json["error"]["message"],
                "attachments exceed 100 MiB in total"
            );
        }
    }

    #[test]
    fn fold_summary_strips_chet_breakdown_unconditionally_and_maps_live_states() {
        // 1. All-needs-user case: no live chet_status needed, yet chet_breakdown is still stripped.
        let mut all_user = vec![json!({
            "group_id": "g_1",
            "state": "fresh",
            "pending": 2,
            "chet_breakdown": {
                "state": "fresh",
                "needs_user": 2,
                "waiting": 0,
                "waiting_unregistered": 0,
                "dispatched_input_ids": [],
            }
        })];
        assert!(!summary_needs_live_chet_status(&all_user));
        fold_summary_chet_breakdowns(&mut all_user, true, None, true);
        assert!(all_user[0].get("chet_breakdown").is_none());
        assert_eq!(
            all_user[0]["unattended"],
            json!({"state": "fresh", "needs_user": 2, "with_chet": 0, "unknown": 0})
        );

        // 2. Non-admin strips chet_breakdown and never exposes unattended.
        let mut non_admin = vec![json!({
            "group_id": "g_1",
            "state": "fresh",
            "pending": 1,
            "chet_breakdown": {
                "state": "fresh",
                "needs_user": 0,
                "waiting": 1,
                "waiting_unregistered": 0,
                "dispatched_input_ids": [],
            }
        })];
        fold_summary_chet_breakdowns(&mut non_admin, false, None, true);
        assert!(non_admin[0].get("chet_breakdown").is_none());
        assert!(non_admin[0].get("unattended").is_none());

        // 3. Mixed live states: preparing/queued/working -> with_chet; orphan dispatched -> unknown.
        let mut mixed = vec![json!({
            "group_id": "g_2",
            "state": "fresh",
            "pending": 6,
            "chet_breakdown": {
                "state": "fresh",
                "needs_user": 1,
                "waiting": 1,
                "waiting_unregistered": 1,
                "dispatched_input_ids": ["id-prep", "id-work", "id-orphan"],
            }
        })];
        assert!(summary_needs_live_chet_status(&mixed));
        let status = json!({
            "preparing": ["id-prep"],
            "queued": [],
            "working": ["id-work"],
            "owner_busy": false,
            "history_write_failed": false,
            "start_problem": null,
        });
        fold_summary_chet_breakdowns(&mut mixed, true, Some(&status), true);
        assert!(mixed[0].get("chet_breakdown").is_none());
        assert_eq!(
            mixed[0]["unattended"],
            json!({"state": "fresh", "needs_user": 1, "with_chet": 4, "unknown": 1})
        );

        // 4. Start problem turns waiting + waiting_unregistered into unknown (never with_chet, never needs_user).
        let mut blocked = vec![json!({
            "group_id": "g_3",
            "state": "fresh",
            "pending": 2,
            "chet_breakdown": {
                "state": "fresh",
                "needs_user": 0,
                "waiting": 1,
                "waiting_unregistered": 1,
                "dispatched_input_ids": [],
            }
        })];
        let failed_status = json!({
            "preparing": [],
            "queued": [],
            "working": [],
            "owner_busy": false,
            "history_write_failed": false,
            "start_problem": {"code": "start_failed"},
        });
        fold_summary_chet_breakdowns(&mut blocked, true, Some(&failed_status), true);
        assert_eq!(
            blocked[0]["unattended"],
            json!({"state": "fresh", "needs_user": 0, "with_chet": 0, "unknown": 2})
        );

        // 5. owner_busy turns non-preparing dispatched IDs into unknown; corrupt breakdown -> unknown.
        let mut busy_and_corrupt = vec![
            json!({
                "group_id": "g_4",
                "state": "fresh",
                "pending": 2,
                "chet_breakdown": {
                    "state": "fresh",
                    "needs_user": 0,
                    "waiting": 0,
                    "waiting_unregistered": 0,
                    "dispatched_input_ids": ["id-prep", "id-maybe-working"],
                }
            }),
            json!({
                "group_id": "g_5",
                "state": "fresh",
                "pending": 3,
                "chet_breakdown": {
                    "state": "unknown",
                    "error": "chet_history_unreadable",
                }
            }),
        ];
        let busy_status = json!({
            "preparing": ["id-prep"],
            "queued": [],
            "working": [],
            "owner_busy": true,
            "history_write_failed": false,
            "start_problem": null,
        });
        fold_summary_chet_breakdowns(&mut busy_and_corrupt, true, Some(&busy_status), true);
        assert_eq!(
            busy_and_corrupt[0]["unattended"],
            json!({"state": "fresh", "needs_user": 0, "with_chet": 1, "unknown": 1})
        );
        assert_eq!(
            busy_and_corrupt[1]["unattended"],
            json!({"state": "unknown", "needs_user": null, "with_chet": null, "unknown": null})
        );
    }
}

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::api::{ApiError, ApiResult, call};

const MAX_INLINE_MESSAGE_TEXT_BYTES: usize = 64 * 1024;

pub(super) async fn dispatch(
    state: &AppState,
    group_id: String,
    operation: &str,
    args: &mut Map<String, Value>,
) -> ApiResult {
    args.insert("group_id".into(), Value::String(group_id.clone()));
    let Some(text) = args
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| text.len() > MAX_INLINE_MESSAGE_TEXT_BYTES)
        .map(str::to_owned)
    else {
        return call(state, operation, args.clone()).await;
    };
    if args
        .get("attachments")
        .is_some_and(|attachments| !attachments.is_array())
    {
        return Err(ApiError::bad_code(
            "invalid_attachments",
            "attachments must be an array",
            json!({}),
        ));
    }
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    let digest = digest.get(..12).unwrap_or(&digest);
    let title = format!("cccc-message-{digest}.txt");
    args.insert("text".into(), Value::String(format!("[file] {title}")));
    let mut preflight_args = args.clone();
    preflight_args.insert("operation".into(), Value::String(operation.into()));
    preflight_args.insert("has_attachments".into(), Value::Bool(true));
    let axum::Json(preflight_response) =
        call(state, "message_upload_preflight", preflight_args).await?;
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
        return Ok(axum::Json(json!({"ok":true,"result":result})));
    }

    let blob = cccc_core::blobs::store(&state.home, &group_id, text.as_bytes())
        .map_err(|error| ApiError::bad(error.to_string()))?;
    let attachment = json!({
        "kind":"file",
        "path":blob.path,
        "title":title,
        "mime_type":"text/plain;charset=utf-8",
        "bytes":blob.bytes,
        "sha256":blob.sha256,
    });
    match args.entry("attachments").or_insert_with(|| json!([])) {
        Value::Array(attachments) => attachments.push(attachment),
        _ => unreachable!("attachments were validated before preflight"),
    }
    call(state, operation, args.clone()).await
}

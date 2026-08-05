use axum::Json;
use axum::extract::{Path, State};
use serde_json::{Value, json};

use super::{assistant, load, voice_asr};
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

pub(super) async fn install(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_runtime_id(&body)?;
    let runtime = voice_asr::runtime_status();
    Ok(success(json!({
        "group_id":group_id,
        "assistant":assistant(&load(&state,&group_id)?),
        "service_runtime":runtime,
        "effect":{"changed":false,"kind":"already_installed"}
    })))
}

pub(super) async fn remove(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_runtime_id(&body)?;
    let _ = load(&state, &group_id)?;
    Err(ApiError::conflict(
        "voice_runtime_not_removable",
        "the native Rust voice runtime is linked into the CCCC binary",
        json!({"runtime_id":"sherpa_onnx_streaming"}),
    ))
}

fn validate_runtime_id(body: &Value) -> Result<(), ApiError> {
    let runtime_id = body
        .get("runtime_id")
        .and_then(Value::as_str)
        .unwrap_or("sherpa_onnx_streaming");
    if runtime_id == "sherpa_onnx_streaming" {
        Ok(())
    } else {
        Err(ApiError::not_found_code(
            "voice_runtime_unknown",
            format!("unknown voice runtime: {runtime_id}"),
        ))
    }
}

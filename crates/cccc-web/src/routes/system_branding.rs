use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, call, object, success};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/branding", get(get_branding).put(update_branding))
}

async fn get_branding(State(state): State<AppState>) -> ApiResult {
    let response = call(&state, "branding_get", Default::default()).await?;
    Ok(payload_response(&response.0["result"]["branding"]))
}

async fn update_branding(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let before = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let mut patch = serde_json::Map::new();
    let mut cleanup = Vec::new();
    if let Some(value) = body.get("product_name") {
        patch.insert("product_name".into(), value.clone());
    }
    for (flag, key) in [
        ("clear_logo_icon", "logo_icon_asset_path"),
        ("clear_favicon", "favicon_asset_path"),
    ] {
        if body.get(flag).and_then(Value::as_bool).unwrap_or(false) {
            if let Some(relative) = before.branding.get(key).and_then(Value::as_str) {
                cleanup.push(relative.to_owned());
            }
            patch.insert(key.into(), Value::String(String::new()));
        }
    }
    let response = call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await?;
    for relative in cleanup {
        cccc_core::branding::delete(&state.home, &relative).map_err(|error| {
            crate::api::ApiError::bad_code(
                "branding_cleanup_failed",
                error.to_string(),
                json!({"path":relative}),
            )
        })?;
    }
    Ok(payload_response(&response.0["result"]["branding"]))
}

pub(super) fn payload_response(value: &Value) -> axum::Json<Value> {
    let raw = value.as_object().cloned().unwrap_or_default();
    success(json!({"branding":cccc_core::branding::payload(&raw)}))
}

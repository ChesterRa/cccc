//! Web mods: the list of installed mods (with a revision pages poll to pick up
//! changes), their files, and the enabled switch. Reads need any signed-in viewer;
//! every write needs an administrator (see `auth::requires_admin`).

use axum::extract::{Path, State};
use axum::http::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web_mods", get(list_mods))
        .route("/api/v1/web_mods/files/{id}/{*path}", get(mod_file))
        .route("/api/v1/web_mods/{id}/enabled", put(set_enabled))
}

fn listing(state: &AppState) -> Result<Value, ApiError> {
    let mods =
        cccc_core::web_mods::list(&state.home).map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(json!({
        "api": cccc_core::web_mods::API_VERSION,
        "revision": cccc_core::web_mods::revision(&mods),
        "mods": mods,
    }))
}

async fn list_mods(State(state): State<AppState>) -> ApiResult {
    Ok(success(listing(&state)?))
}

async fn mod_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response<axum::body::Body>, ApiError> {
    let file = cccc_core::web_mods::resolve_file(&state.home, &id, &path)
        .map_err(|_| ApiError::not_found("mod file not found"))?;
    let mime = mime_guess::from_path(&file)
        .first_or_octet_stream()
        .to_string();
    super::file_response::stream(&file, &mime, Some("no-cache"), None)
        .await
        .map_err(|_| ApiError::not_found("mod file not found"))
}

async fn set_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return Err(ApiError::bad("enabled must be true or false"));
    };
    cccc_core::web_mods::set_enabled(&state.home, &id, enabled).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError::not_found("mod not found")
        } else {
            ApiError::bad(error.to_string())
        }
    })?;
    Ok(success(listing(&state)?))
}

#[cfg(test)]
#[path = "web_mods_tests.rs"]
mod tests;

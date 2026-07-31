use axum::Router;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::Response;
use axum::routing::get;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, call, object};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/branding/assets/{asset_kind}",
            get(get_asset).post(upload_asset).delete(delete_asset),
        )
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024))
}

async fn get_asset(
    State(state): State<AppState>,
    Path(kind): Path<String>,
) -> Result<Response<axum::body::Body>, crate::api::ApiError> {
    let global = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let relative = cccc_core::branding::asset_relative(&global.branding, &kind)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    if relative.is_empty() {
        return Err(crate::api::ApiError::not_found(
            "custom branding asset not found",
        ));
    }
    let path = cccc_core::branding::resolve(&state.home, &relative)
        .map_err(|error| crate::api::ApiError::not_found(error.to_string()))?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    super::file_response::stream(&path, &mime, Some("no-cache"), None)
        .await
        .map_err(|error| crate::api::ApiError::not_found(error.to_string()))
}

async fn upload_asset(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut bytes = None;
    let mut mime = String::new();
    let mut filename = String::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?
    {
        if field.name() == Some("file") {
            mime = field.content_type().unwrap_or("").into();
            filename = field.file_name().unwrap_or("asset").into();
            bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|error| crate::api::ApiError::bad(error.to_string()))?
                    .to_vec(),
            );
        }
    }
    let before = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let stored = cccc_core::branding::store(
        &state.home,
        &kind,
        &bytes.ok_or_else(|| crate::api::ApiError::bad("file is required"))?,
        &mime,
        &filename,
    )
    .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let key = format!("{kind}_asset_path");
    let old = before
        .branding
        .get(&key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut patch = serde_json::Map::new();
    patch.insert(key, Value::String(stored.rel_path.clone()));
    let response = match call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            if let Err(rollback) = cccc_core::branding::delete(&state.home, &stored.rel_path) {
                return Err(crate::api::ApiError::bad_code(
                    "rollback_failed",
                    format!("{error}; failed to remove staged branding asset: {rollback}"),
                    json!({"path":stored.rel_path}),
                ));
            }
            return Err(error);
        }
    };
    if !old.is_empty() && old != stored.rel_path {
        cccc_core::branding::delete(&state.home, &old).map_err(|error| {
            crate::api::ApiError::bad_code(
                "branding_cleanup_failed",
                error.to_string(),
                json!({"path":old}),
            )
        })?;
    }
    Ok(super::system_branding::payload_response(
        &response.0["result"]["branding"],
    ))
}

async fn delete_asset(State(state): State<AppState>, Path(kind): Path<String>) -> ApiResult {
    let global = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let relative = cccc_core::branding::asset_relative(&global.branding, &kind)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let mut patch = serde_json::Map::new();
    patch.insert(format!("{kind}_asset_path"), Value::String(String::new()));
    let response = call(
        &state,
        "branding_update",
        object(json!({"by":"user","patch":patch})),
    )
    .await?;
    cccc_core::branding::delete(&state.home, &relative).map_err(|error| {
        crate::api::ApiError::bad_code(
            "branding_cleanup_failed",
            error.to_string(),
            json!({"path":relative}),
        )
    })?;
    Ok(super::system_branding::payload_response(
        &response.0["result"]["branding"],
    ))
}

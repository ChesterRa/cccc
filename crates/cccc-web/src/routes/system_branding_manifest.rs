use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::AppState;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/ui/manifest.webmanifest", get(manifest))
        .route("/pwa-icon.svg", get(pwa_icon))
        .route("/pwa-icon-maskable.svg", get(pwa_icon_maskable))
        .route("/apple-touch-icon.png", get(apple_touch_icon))
}

async fn manifest(State(state): State<AppState>) -> Result<Response, crate::api::ApiError> {
    let settings = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let mut response =
        Json(cccc_core::branding::web_app_manifest(&settings.branding)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/manifest+json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

async fn pwa_icon(State(state): State<AppState>) -> Result<Response, crate::api::ApiError> {
    pwa_icon_response(&state, false)
}

async fn pwa_icon_maskable(
    State(state): State<AppState>,
) -> Result<Response, crate::api::ApiError> {
    pwa_icon_response(&state, true)
}

fn pwa_icon_response(state: &AppState, maskable: bool) -> Result<Response, crate::api::ApiError> {
    let settings = cccc_core::settings::load(&state.home).map_err(|error| {
        tracing::warn!(%error, maskable, "failed to load settings for public branding icon");
        crate::api::ApiError::not_found("branding icon unavailable")
    })?;
    let bytes = cccc_core::branding::pwa_icon_svg(&state.home, &settings.branding, maskable)
        .map_err(|error| {
            tracing::warn!(%error, maskable, "failed to render public branding icon");
            crate::api::ApiError::not_found("branding icon unavailable")
        })?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response())
}

async fn apple_touch_icon(State(state): State<AppState>) -> Result<Response, crate::api::ApiError> {
    let settings = cccc_core::settings::load(&state.home)
        .map_err(|error| crate::api::ApiError::bad(error.to_string()))?;
    let target = cccc_core::branding::apple_touch_icon_url(&state.home, &settings.branding);
    let mut response = Redirect::temporary(&target).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

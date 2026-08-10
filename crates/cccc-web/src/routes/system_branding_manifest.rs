use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::AppState;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/ui/manifest.webmanifest", get(manifest))
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

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::{Json, Router};
use cccc_core::nomcp::{Session, Store};
use serde_json::{Value, json};

use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(super::nomcp_admin::routes())
        .merge(super::nomcp_pages::routes())
        .merge(super::nomcp_send::routes())
}

pub(super) fn authorize(
    state: &AppState,
    sid: &str,
    token: &str,
) -> Result<(Session, std::path::PathBuf), String> {
    let store = Store::new(state.home.clone()).map_err(|error| error.to_string())?;
    let session = store
        .authorize(sid, token)
        .map_err(|error| error.to_string())?;
    let root = super::nomcp_resources::root(&state.home, &session)?;
    Ok((session, root))
}

pub(super) fn auth_failure(state: &AppState, sid: &str, token: &str) -> Response {
    let error = Store::new(state.home.clone())
        .and_then(|store| store.authorize(sid, token))
        .err();
    let status = error
        .as_ref()
        .map(|error| error.to_string())
        .filter(|text| text.contains("revoked") || text.contains("expired"))
        .map_or(StatusCode::FORBIDDEN, |_| StatusCode::GONE);
    failure(
        status,
        error.map_or_else(
            || "session authorization failed".into(),
            |error| error.to_string(),
        ),
    )
}

pub(super) fn formatted(title: &str, value: Value, format: Option<&str>) -> Response {
    if format == Some("json") {
        Json(value).into_response()
    } else {
        Html(super::nomcp_render::html(
            title,
            &serde_json::to_string_pretty(&value).unwrap_or_default(),
        ))
        .into_response()
    }
}

pub(super) fn page(title: &str, body: &str, format: Option<&str>) -> Response {
    if matches!(format, Some("md" | "markdown")) {
        (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/markdown; charset=utf-8",
            )],
            body.to_owned(),
        )
            .into_response()
    } else {
        Html(super::nomcp_render::html(title, body)).into_response()
    }
}

pub(super) fn failure(status: StatusCode, error: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({"ok":false,"error":{"code":"nomcp_error","message":error.to_string(),"details":{}}})),
    )
        .into_response()
}

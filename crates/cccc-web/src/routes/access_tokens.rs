use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use super::access_token_support::{
    clean_groups, cookie, error, mask, server_error, store, valid_id,
};
use crate::AppState;
use crate::auth::Principal;

#[derive(Debug, Deserialize)]
struct CreateBody {
    user_id: String,
    #[serde(default)]
    allowed_groups: Vec<String>,
    #[serde(default)]
    is_admin: bool,
    custom_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateBody {
    allowed_groups: Option<Vec<String>>,
    is_admin: Option<bool>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/access-tokens", get(list).post(create))
        .route(
            "/api/v1/access-tokens/{token_id}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/api/v1/access-tokens/{token_id}/reveal", get(reveal))
        .route("/api/v1/web_access/session", get(web_session))
        .route("/api/v1/web_access/logout", axum::routing::post(logout))
}

async fn list(State(state): State<AppState>) -> Response {
    let store = match store(&state) {
        Ok(store) => store,
        Err(error_value) => return server_error(error_value),
    };
    match store.list() {
        Ok(items) => Json(json!({"ok":true,"result":{"access_tokens":items.iter().map(mask).collect::<Vec<_>>()}})).into_response(),
        Err(error) => server_error(error),
    }
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> Response {
    let store = match store(&state) {
        Ok(store) => store,
        Err(error_value) => return server_error(error_value),
    };
    let existing = match store.list() {
        Ok(items) => items,
        Err(error) => return server_error(error),
    };
    let groups = clean_groups(body.allowed_groups);
    if existing.is_empty() && !body.is_admin {
        return error(
            StatusCode::BAD_REQUEST,
            "admin_required_first",
            "the first access token must be an administrator",
        );
    }
    if !body.is_admin && groups.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "scoped access tokens require at least one group",
        );
    }
    match store.create(
        &body.user_id,
        groups,
        body.is_admin,
        body.custom_token.as_deref(),
    ) {
        Ok(token) => {
            let body = Json(json!({"ok":true,"result":{"access_token":token}}));
            if existing.is_empty() {
                let secure = headers
                    .get("x-forwarded-proto")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.eq_ignore_ascii_case("https"));
                return ([(header::SET_COOKIE, cookie(&token.token, secure))], body)
                    .into_response();
            }
            body.into_response()
        }
        Err(error_value) => error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &error_value.to_string(),
        ),
    }
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Response {
    if !valid_id(&id) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid token_id",
        );
    }
    let groups = body.allowed_groups.map(clean_groups);
    let store = match store(&state) {
        Ok(store) => store,
        Err(error_value) => return server_error(error_value),
    };
    let current = match store.list() {
        Ok(tokens) => tokens.into_iter().find(|token| token.token_id() == id),
        Err(error_value) => return server_error(error_value),
    };
    let Some(current) = current else {
        return error(StatusCode::NOT_FOUND, "not_found", "access token not found");
    };
    let next_admin = body.is_admin.unwrap_or(current.is_admin);
    let effective_groups = groups.clone().unwrap_or(current.allowed_groups);
    if !next_admin && effective_groups.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "scoped access tokens require at least one group",
        );
    }
    match store.update(&id, Some(effective_groups), body.is_admin) {
        Ok(Some(token)) => {
            Json(json!({"ok":true,"result":{"access_token":mask(&token)}})).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "access token not found"),
        Err(error_value) => server_error(error_value),
    }
}

async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    if !valid_id(&id) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid token_id",
        );
    }
    let store = match store(&state) {
        Ok(store) => store,
        Err(error_value) => return server_error(error_value),
    };
    match store.delete(&id) {
        Ok(Some(token)) => {
            let remain = store.list().map_or(true, |items| !items.is_empty());
            Json(json!({"ok":true,"result":{"deleted":true,"access_tokens_remain":remain,"deleted_current_session":token.token==principal.raw_token}})).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "access token not found"),
        Err(error_value) => server_error(error_value),
    }
}

async fn reveal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if !valid_id(&id) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid token_id",
        );
    }
    let store = match store(&state) {
        Ok(store) => store,
        Err(error_value) => return server_error(error_value),
    };
    match store.list() {
        Ok(tokens) => tokens
            .into_iter()
            .find(|token| token.token_id() == id)
            .map_or_else(
                || error(StatusCode::NOT_FOUND, "not_found", "access token not found"),
                |token| Json(json!({"ok":true,"result":{"token":token.token}})).into_response(),
            ),
        Err(error_value) => server_error(error_value),
    }
}

async fn web_session(principal: Option<Extension<Principal>>) -> Json<Value> {
    let principal = principal.map(|value| value.0);
    Json(json!({"ok":true,"result":{"web_access_session":{
        "current_browser_signed_in":principal.is_some(),
        "can_access_global_settings":principal.as_ref().is_some_and(|item| item.is_admin),
        "user_id":principal.map(|item| item.user_id).unwrap_or_default()
    }}}))
}

async fn logout() -> Response {
    let cookie = "cccc_access_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    (
        [(header::SET_COOKIE, cookie)],
        Json(json!({"ok":true,"result":{"signed_out":true}})),
    )
        .into_response()
}

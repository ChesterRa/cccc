use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cccc_core::access_tokens::{AccessTokenStore, is_last_admin_required};
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
    bootstrap_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateBody {
    allowed_groups: Option<Vec<String>>,
    is_admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ExchangeQuery {
    #[serde(default)]
    code: String,
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
        .route("/api/v1/web_access/exchange", get(exchange))
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
    let first_admin = !existing.iter().any(|token| token.is_admin);
    if first_admin {
        if !body.is_admin {
            return error(
                StatusCode::BAD_REQUEST,
                "admin_required_first",
                "the first access token must be an administrator",
            );
        }
        let supplied = body.bootstrap_token.as_deref().unwrap_or_default();
        match cccc_core::web_bootstrap::consume_web_bootstrap_token(&state.home, supplied) {
            Ok(true) => {}
            Ok(false) => {
                return error(
                    StatusCode::UNAUTHORIZED,
                    "bootstrap_required",
                    "a valid local Web bootstrap code is required",
                );
            }
            Err(error_value) => return server_error(error_value),
        }
    }
    let groups = clean_groups(body.allowed_groups);
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
            if first_admin {
                let secure = crate::request_origin::is_https(&state, &headers);
                return ([(header::SET_COOKIE, cookie(&token.token, secure))], body)
                    .into_response();
            }
            body.into_response()
        }
        Err(error_value) => {
            if first_admin {
                let _ = cccc_core::web_bootstrap::ensure_web_bootstrap_token(&state.home);
            }
            error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &error_value.to_string(),
            )
        }
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
        Err(error_value) if is_last_admin_required(&error_value) => error(
            StatusCode::BAD_REQUEST,
            "last_admin_required",
            error_value
                .to_string()
                .strip_prefix("last_admin_required: ")
                .unwrap_or("an administrator access token is required"),
        ),
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
        Err(error_value) if is_last_admin_required(&error_value) => error(
            StatusCode::BAD_REQUEST,
            "last_admin_required",
            error_value
                .to_string()
                .strip_prefix("last_admin_required: ")
                .unwrap_or("an administrator access token is required"),
        ),
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

async fn web_session(
    State(state): State<AppState>,
    principal: Option<Extension<Principal>>,
) -> Json<Value> {
    let principal = principal.map(|value| value.0);
    let access_tokens = AccessTokenStore::new(state.home.clone())
        .and_then(|store| store.list())
        .unwrap_or_default();
    let access_token_count = access_tokens.len();
    let bootstrap_required = !access_tokens.iter().any(|token| token.is_admin);
    if bootstrap_required {
        let _ = cccc_core::web_bootstrap::ensure_web_bootstrap_token(&state.home);
    }
    let runtime_visibility = runtime_visibility(&state.home);
    let authenticated = principal.is_some();
    let disclose_details = principal.as_ref().is_some_and(|item| item.is_admin);
    Json(json!({"ok":true,"result":{"web_access_session":{
        "login_active": access_token_count > 0,
        "current_browser_signed_in":authenticated,
        "access_token_count":if disclose_details {access_token_count}else{0},
        "bootstrap_required":bootstrap_required,
        "can_access_global_settings":bootstrap_required || principal.as_ref().is_some_and(|item| item.is_admin),
        "user_id":principal.map(|item| item.user_id).unwrap_or_default(),
        "runtime_visibility":if authenticated {runtime_visibility}else{json!({})}
    }}}))
}

async fn exchange(
    State(state): State<AppState>,
    Query(query): Query<ExchangeQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(origin) = crate::request_origin::served_origin(&state, &headers) else {
        return error(
            StatusCode::UNAUTHORIZED,
            "web_login_grant_invalid",
            "Web login link is invalid or expired",
        );
    };
    let token_id = match cccc_core::web_login_grants::consume(&state.home, &query.code, &origin) {
        Ok(Some(token_id)) => token_id,
        Ok(None) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "web_login_grant_invalid",
                "Web login link is invalid or expired",
            );
        }
        Err(error_value) => return server_error(error_value),
    };
    let token = match AccessTokenStore::new(state.home.clone()).and_then(|store| store.list()) {
        Ok(tokens) => tokens
            .into_iter()
            .find(|token| token.is_admin && token.token_id() == token_id),
        Err(error_value) => return server_error(error_value),
    };
    let Some(token) = token else {
        return error(
            StatusCode::UNAUTHORIZED,
            "web_login_grant_invalid",
            "Web login link is invalid or expired",
        );
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        axum::http::HeaderValue::from_static("/ui/"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    if let Ok(value) = axum::http::HeaderValue::from_str(&cookie(
        &token.token,
        crate::request_origin::is_https(&state, &headers),
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn runtime_visibility(home: &cccc_core::HomeLayout) -> Value {
    let settings = cccc_core::settings::load(home).unwrap_or_default();
    let visibility = settings
        .observability
        .get("runtime_visibility")
        .and_then(Value::as_object);
    json!({
        "peer_runtime": visibility
            .and_then(|value| value.get("peer_runtime"))
            .and_then(Value::as_str)
            .unwrap_or("visible"),
        "assistant_runtime": visibility
            .and_then(|value| value.get("assistant_runtime"))
            .and_then(Value::as_str)
            .unwrap_or("hidden")
    })
}

async fn logout() -> Response {
    let cookie = "cccc_access_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
    (
        [(header::SET_COOKIE, cookie)],
        Json(json!({"ok":true,"result":{"signed_out":true}})),
    )
        .into_response()
}

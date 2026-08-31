use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cccc_core::access_tokens::{AccessToken, AccessTokenStore};
use percent_encoding::percent_decode_str;
use serde_json::json;

use crate::AppState;
use crate::routes::access_token_support::cookie;

#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: String,
    pub allowed_groups: Vec<String>,
    pub is_admin: bool,
    pub raw_token: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenSource {
    None,
    Bearer,
    Cookie,
    Local,
}

impl Principal {
    fn from_token(token: AccessToken) -> Self {
        Self {
            user_id: token.user_id,
            allowed_groups: token.allowed_groups,
            is_admin: token.is_admin,
            raw_token: token.token,
        }
    }

    pub fn allows(&self, group_id: &str) -> bool {
        self.is_admin || self.allowed_groups.iter().any(|item| item == group_id)
    }
}

pub async fn authorize(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !websocket_origin_allowed(&state, &request) {
        tracing::warn!(
            origin = request
                .headers()
                .get(header::ORIGIN)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default(),
            served_origin = ?crate::request_origin::served_origin(&state, request.headers()),
            path = request.uri().path(),
            "rejected WebSocket origin"
        );
        return failure_text(
            StatusCode::FORBIDDEN,
            "origin_not_allowed",
            "WebSocket origin is not allowed",
        );
    }
    let store = match AccessTokenStore::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return auth_store_failure(error),
    };
    let tokens = match store.list() {
        Ok(tokens) => tokens,
        Err(error) => return auth_store_failure(error),
    };
    let has_admin = tokens.iter().any(|token| token.is_admin);
    if is_first_admin_bootstrap(request.method(), request.uri().path()) && !has_admin {
        return next.run(request).await;
    }
    let secure_cookie = crate::request_origin::is_https(&state, request.headers());
    let (raw, mut token_source) = request_token(&request);
    let mut principal = match store.lookup(&raw) {
        Ok(Some(token)) => Some(Principal::from_token(token)),
        Ok(None) => None,
        Err(error) => return auth_store_failure(error),
    };
    if principal.is_none()
        && accepts_local_principal(request.method(), request.uri().path())
        && crate::local_browser_auth::allowed(&state, &request)
    {
        token_source = TokenSource::Local;
        principal = Some(Principal {
            user_id: "local".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: String::new(),
        });
    }
    if tokens.is_empty() && principal.is_none() {
        if is_public(request.method(), request.uri().path()) {
            return next.run(request).await;
        }
        return failure_text(
            StatusCode::UNAUTHORIZED,
            "bootstrap_required",
            "remote Web access requires an administrator access token",
        );
    }
    if principal.is_some()
        && !is_public(request.method(), request.uri().path())
        && matches!(token_source, TokenSource::Cookie | TokenSource::Local)
        && is_unsafe_method(request.method())
        && !crate::request_origin::cookie_csrf_allowed(&state, request.headers())
    {
        return failure_text(
            StatusCode::FORBIDDEN,
            "csrf_origin_invalid",
            "Cookie-authenticated write requests require an allowed Origin or Referer",
        );
    }
    let bootstrap_cookie = principal.as_ref().and_then(|principal| {
        (request.uri().path() == "/api/v1/web_access/session" && !principal.raw_token.is_empty())
            .then(|| cookie(&principal.raw_token, secure_cookie))
    });
    if is_public(request.method(), request.uri().path()) {
        if let Some(principal) = principal {
            request.extensions_mut().insert(principal);
        }
        return with_bootstrap_cookie(next.run(request).await, bootstrap_cookie.as_deref());
    }
    let Some(principal) = principal else {
        return failure_text(
            StatusCode::UNAUTHORIZED,
            "auth_required",
            "valid access token required",
        );
    };
    if requires_admin(request.method(), request.uri().path()) && !principal.is_admin {
        return failure_text(
            StatusCode::FORBIDDEN,
            "admin_required",
            "administrator access required",
        );
    }
    if let Some(group_id) = group_from_path(request.uri().path())
        && !principal.allows(group_id)
    {
        return failure_text(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "group access denied",
        );
    }
    if request.uri().path() == "/api/v1/debug/snapshot" && !principal.is_admin {
        let Some(group_id) = group_from_query(&request) else {
            return failure_text(
                StatusCode::FORBIDDEN,
                "permission_denied",
                "group access denied",
            );
        };
        if !principal.allows(&group_id) {
            return failure_text(
                StatusCode::FORBIDDEN,
                "permission_denied",
                "group access denied",
            );
        }
    }
    request.extensions_mut().insert(principal);
    with_bootstrap_cookie(next.run(request).await, bootstrap_cookie.as_deref())
}

fn websocket_origin_allowed(state: &AppState, request: &Request) -> bool {
    websocket_origin_allowed_with_proxy(
        request,
        crate::request_origin::proxy_headers_trusted(state),
    )
}

fn websocket_origin_allowed_with_proxy(request: &Request, trust_proxy: bool) -> bool {
    let websocket = request
        .headers()
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if !websocket {
        return true;
    }
    let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    crate::request_origin::origin_allowed_with_proxy(request.headers(), origin, trust_proxy)
}

fn with_bootstrap_cookie(mut response: Response, cookie: Option<&str>) -> Response {
    if let Some(cookie) = cookie
        && let Ok(value) = HeaderValue::from_str(cookie)
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

fn request_token(request: &Request) -> (String, TokenSource) {
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(str::trim)
        .unwrap_or("");
    if !bearer.is_empty() {
        return (bearer.into(), TokenSource::Bearer);
    }
    let cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                cookie
                    .trim()
                    .strip_prefix("cccc_access_token=")
                    .map(decode_token)
            })
        });
    cookie.map_or_else(
        || (String::new(), TokenSource::None),
        |token| (token, TokenSource::Cookie),
    )
}

fn is_unsafe_method(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn is_first_admin_bootstrap(method: &Method, path: &str) -> bool {
    *method == Method::POST && path == "/api/v1/access-tokens"
}

fn decode_token(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn is_public(method: &Method, path: &str) -> bool {
    matches!(
        path,
        "/api/v1/ping"
            | "/api/v1/health"
            | "/api/v1/ready"
            | "/api/v1/web_access/session"
            | "/api/v1/web_access/exchange"
    ) || matches!(
        path,
        "/api/group-bridge/pairing/requests/remote"
            | "/api/group-bridge/pairing/requests/remote/status"
            | "/api/group-bridge/pairing/requests/remote/claim"
            | "/api/group-bridge/session/send"
            | "/api/group-bridge/session/ws"
            | "/api/group-bridge/session/ws/v2"
    ) || (*method == Method::GET && path == "/api/v1/branding")
        || (matches!(*method, Method::GET | Method::HEAD)
            && path.starts_with("/api/v1/branding/assets/"))
        || !path.starts_with("/api/")
}

fn accepts_local_principal(method: &Method, path: &str) -> bool {
    !is_public(method, path)
        || (matches!(*method, Method::GET | Method::HEAD)
            && matches!(path, "/api/v1/ping" | "/api/v1/web_access/session"))
}

fn requires_admin(method: &Method, path: &str) -> bool {
    path.starts_with("/api/v1/access-tokens")
        || path.starts_with("/api/v1/actor_profiles")
        || path.starts_with("/api/v1/nomcp/")
        || path.starts_with("/api/v1/web-model/")
        || path.starts_with("/api/v1/space/providers/")
        || path == "/api/v1/mcp"
        || path.starts_with("/api/v1/observability")
        || path.starts_with("/api/v1/branding")
        || path.starts_with("/api/v1/fs/")
        || path.starts_with("/api/v1/registry/")
        || path.starts_with("/api/v1/membership")
        || path.starts_with("/api/v1/remote_access")
        || path == "/api/v1/debug/tail_logs"
        || path == "/api/v1/debug/clear_logs"
        || path.starts_with("/api/v1/capabilities/allowlist")
        || path == "/api/v1/capabilities/block"
        || (path == "/api/v1/groups" && *method == Method::POST)
        || (*method == Method::DELETE && group_from_path(path).is_some())
        || path.ends_with("/reset")
}

fn group_from_query(request: &Request) -> Option<String> {
    if request.uri().path() != "/api/v1/debug/snapshot" {
        return None;
    }
    request
        .uri()
        .query()?
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (decode_token(key) == "group_id").then(|| decode_token(value))
        })
        .next_back()
        .filter(|group_id| !group_id.is_empty())
}

fn group_from_path(path: &str) -> Option<&str> {
    let tail = path.strip_prefix("/api/v1/groups/")?;
    let group_id = tail.split('/').next()?;
    group_id.starts_with("g_").then_some(group_id)
}

fn auth_store_failure(error: impl std::fmt::Display) -> Response {
    tracing::error!(%error, "failed to read CCCC access token store");
    failure_text(
        StatusCode::INTERNAL_SERVER_ERROR,
        "auth_store_error",
        "access token store is unavailable",
    )
}

fn failure_text(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({"ok":false,"error":{"code":code,"message":message,"details":{}}})),
    )
        .into_response()
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cccc_core::access_tokens::{AccessToken, AccessTokenStore};
use percent_encoding::percent_decode_str;
use serde_json::json;

use crate::AppState;

#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: String,
    pub allowed_groups: Vec<String>,
    pub is_admin: bool,
    pub raw_token: String,
}

impl Principal {
    fn local_admin() -> Self {
        Self {
            user_id: "local-user".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: String::new(),
        }
    }

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
    let store = match AccessTokenStore::new(state.home.clone()) {
        Ok(store) => store,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "auth_store_error", error),
    };
    let tokens = match store.list() {
        Ok(tokens) => tokens,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "auth_store_error", error),
    };
    if tokens.is_empty() {
        request.extensions_mut().insert(Principal::local_admin());
        return next.run(request).await;
    }
    let raw = request_token(&request);
    let principal = match store.lookup(&raw) {
        Ok(Some(token)) => Some(Principal::from_token(token)),
        Ok(None) => None,
        Err(error) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "auth_store_error", error),
    };
    if is_public(request.method(), request.uri().path()) {
        if let Some(principal) = principal {
            request.extensions_mut().insert(principal);
        }
        return next.run(request).await;
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
    next.run(request).await
}

fn request_token(request: &Request) -> String {
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
        return bearer.into();
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
    cookie.or_else(|| query_token(request)).unwrap_or_default()
}

fn query_token(request: &Request) -> Option<String> {
    request.uri().query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token").then(|| decode_token(value))
    })
}

fn decode_token(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn is_public(method: &Method, path: &str) -> bool {
    matches!(
        path,
        "/api/v1/ping" | "/api/v1/health" | "/api/v1/ready" | "/api/v1/web_access/session"
    ) || matches!(
        path,
        "/api/group-bridge/pairing/requests/remote"
            | "/api/group-bridge/pairing/requests/remote/status"
            | "/api/group-bridge/pairing/requests/remote/claim"
            | "/api/group-bridge/session/send"
            | "/api/group-bridge/session/ws"
    ) || *method == Method::GET
        && (path == "/api/v1/branding" || path.starts_with("/api/v1/branding/assets/"))
        || !path.starts_with("/api/")
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

fn failure(status: StatusCode, code: &str, error: impl std::fmt::Display) -> Response {
    failure_text(status, code, &error.to_string())
}

fn failure_text(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({"ok":false,"error":{"code":code,"message":message,"details":{}}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_profiles_stay_admin_only_while_scoped_profiles_use_user_policy() {
        assert!(!requires_admin(&Method::GET, "/api/v1/profiles"));
        assert!(requires_admin(&Method::POST, "/api/v1/actor_profiles"));
        assert!(requires_admin(
            &Method::GET,
            "/api/v1/actor_profiles/ap_one/env_private"
        ));
        assert!(requires_admin(
            &Method::POST,
            "/api/v1/space/providers/notebooklm/credential"
        ));
        assert!(!requires_admin(&Method::GET, "/api/v1/groups/g_one/actors"));
    }

    #[test]
    fn global_control_plane_routes_require_admin() {
        for (method, path) in [
            (Method::GET, "/api/v1/remote_access"),
            (Method::POST, "/api/v1/remote_access/start"),
            (Method::GET, "/api/v1/debug/tail_logs"),
            (Method::POST, "/api/v1/debug/clear_logs"),
            (Method::GET, "/api/v1/capabilities/allowlist"),
            (Method::POST, "/api/v1/capabilities/allowlist/validate"),
            (Method::POST, "/api/v1/capabilities/block"),
        ] {
            assert!(requires_admin(&method, path), "{method} {path}");
        }
    }
}

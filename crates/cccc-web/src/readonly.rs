use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::AppState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WebMode {
    #[default]
    Normal,
    Exhibit,
}

impl WebMode {
    pub fn from_env() -> Self {
        let mode = std::env::var("CCCC_WEB_MODE").unwrap_or_default();
        let read_only = std::env::var("CCCC_WEB_READONLY").unwrap_or_default();
        Self::from_values(&mode, &read_only)
    }

    fn from_values(mode: &str, read_only: &str) -> Self {
        if matches!(
            mode.trim().to_ascii_lowercase().as_str(),
            "exhibit" | "readonly" | "read-only" | "ro"
        ) || truthy(read_only)
        {
            Self::Exhibit
        } else {
            Self::Normal
        }
    }

    pub(crate) fn is_read_only(self) -> bool {
        self == Self::Exhibit
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Exhibit => "exhibit",
        }
    }
}

pub(crate) fn exhibit_allow_terminal_from_env() -> bool {
    truthy(&std::env::var("CCCC_WEB_EXHIBIT_ALLOW_TERMINAL").unwrap_or_default())
}

pub(crate) async fn reject_socket(mut socket: WebSocket, code: &str, message: &str) {
    let _ = socket
        .send(Message::Text(
            json!({
                "ok": false,
                "error": {"code": code, "message": message, "details": {}}
            })
            .to_string()
            .into(),
        ))
        .await;
    let _ = socket.send(Message::Close(None)).await;
}

pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if state.web_mode.is_read_only() && !is_read_only_safe(request.method(), request.uri().path()) {
        return read_only_response();
    }
    next.run(request).await
}

fn is_read_only_safe(method: &Method, path: &str) -> bool {
    match *method {
        Method::GET | Method::HEAD => !is_mutating_get(path),
        Method::OPTIONS => true,
        _ => false,
    }
}

fn is_mutating_get(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/registry/reconcile" | "/api/v1/fs/scope_root" | "/api/group-bridge/session/ws"
    ) || path
        .strip_prefix("/nomcp/s/")
        .and_then(|rest| rest.strip_suffix("/send"))
        .is_some_and(|session_id| !session_id.is_empty() && !session_id.contains('/'))
}

fn read_only_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(json!({
            "ok": false,
            "error": {
                "code": "read_only",
                "message": "CCCC Web is running in read-only (exhibit) mode.",
                "details": {}
            }
        })),
    )
        .into_response()
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_values_match_legacy_web() {
        for value in ["1", "true", "YES", "y", "on"] {
            assert!(truthy(value));
        }
        assert!(!truthy("false"));
        assert_eq!(WebMode::from_values("exhibit", ""), WebMode::Exhibit);
        assert_eq!(WebMode::from_values("normal", "yes"), WebMode::Exhibit);
        assert_eq!(WebMode::from_values("normal", "0"), WebMode::Normal);
        assert_eq!(WebMode::Normal.as_str(), "normal");
        assert_eq!(WebMode::Exhibit.as_str(), "exhibit");
    }

    #[test]
    fn read_only_safety_blocks_mutating_get_routes() {
        for path in [
            "/api/v1/registry/reconcile",
            "/nomcp/s/session-1/send",
            "/api/group-bridge/session/ws",
        ] {
            assert!(!is_read_only_safe(&Method::GET, path), "{path}");
        }
        assert!(!is_read_only_safe(
            &Method::HEAD,
            "/api/v1/registry/reconcile"
        ));
        assert!(is_read_only_safe(&Method::GET, "/api/v1/ping"));
        assert!(is_read_only_safe(
            &Method::GET,
            "/nomcp/s/session-1/context"
        ));
    }
}

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;

const HSTS_VALUE: &str = "max-age=31536000";

pub async fn apply(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let secure = crate::request_origin::is_https(&state, request.headers());
    let (ancestor, mut response) = match crate::connect_frames::frame_ancestor(&state, &request) {
        Ok(ancestor) => (ancestor, next.run(request).await),
        Err(error) => (None, error.into_response()),
    };
    let headers = response.headers_mut();
    // Multiple CSP policies are enforced together. Preserve stricter resource
    // policies (such as sandboxed workspace SVG) while adding frame authority.
    headers.append(
        header::HeaderName::from_static("content-security-policy"),
        ancestor
            .as_ref()
            .and_then(|origin| {
                HeaderValue::from_str(&format!("frame-ancestors 'self' {origin}")).ok()
            })
            .unwrap_or_else(|| HeaderValue::from_static("frame-ancestors 'self'")),
    );
    if ancestor.is_none() {
        headers.insert(
            header::HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("SAMEORIGIN"),
        );
    }
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(self), microphone=(self), geolocation=()"),
    );
    if secure {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS_VALUE),
        );
    }
    response
}

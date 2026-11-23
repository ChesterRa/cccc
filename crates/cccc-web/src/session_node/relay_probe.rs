//! Native iroh latency/captive-portal probes are required when UDP is unavailable.
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use iroh_relay::server::Handlers;

pub(super) fn response(path: &str, request: &HeaderMap) -> (StatusCode, HeaderMap) {
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    if path == "/generate_204" {
        if let Some(value) = request.get("x-iroh-challenge")
            && !value.is_empty()
            && value.len() < 64
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            && let Ok(value) = value.to_str()
            && let Ok(value) = HeaderValue::from_str(&format!("response {value}"))
        {
            headers.insert("x-iroh-response", value);
        }
        (StatusCode::NO_CONTENT, headers)
    } else {
        headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
        (StatusCode::OK, headers)
    }
}

pub(super) fn handlers() -> Handlers {
    let mut handlers = Handlers::default();
    for path in ["/ping", "/generate_204"] {
        handlers.insert(
            (Method::GET, path),
            Box::new(move |request, builder| {
                let (status, headers) = response(path, request.headers());
                let mut builder = builder.status(status);
                for (name, value) in &headers {
                    builder = builder.header(name, value);
                }
                let body: Box<
                    dyn axum::body::HttpBody<
                            Data = axum::body::Bytes,
                            Error = std::convert::Infallible,
                        > + Send
                        + Unpin,
                > = Box::new(http_body_util::Empty::<axum::body::Bytes>::new());
                Ok(builder.body(body)?)
            }),
        );
    }
    handlers
}

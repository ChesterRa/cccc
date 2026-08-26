use axum::http::{HeaderMap, header};

fn first_list_value(value: &str) -> Option<String> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn forwarded_parameter(headers: &HeaderMap, name: &str) -> Option<String> {
    let first = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())?
        .split(',')
        .next()?;
    first.split(';').find_map(|part| {
        let (key, raw_value) = part.split_once('=')?;
        if !key.trim().eq_ignore_ascii_case(name) {
            return None;
        }
        let value = raw_value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value)
            .trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn forwarded_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(first_list_value)
        .or_else(|| forwarded_parameter(headers, "host"))
}

fn forwarded_scheme(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(first_list_value)
        .or_else(|| forwarded_parameter(headers, "proto"))
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| matches!(value.as_str(), "http" | "https"))
}

pub fn served_origin(headers: &HeaderMap) -> Option<String> {
    let host = forwarded_host(headers).or_else(|| {
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })?;
    if host.is_empty() {
        return None;
    }
    let scheme = forwarded_scheme(headers).unwrap_or_else(|| "http".into());
    cccc_core::web_login_grants::normalize_origin(&format!("{scheme}://{host}"))
}

pub fn source_origin(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return cccc_core::web_login_grants::normalize_origin(origin);
    }
    headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(cccc_core::web_login_grants::normalize_origin)
}

pub fn origin_allowed(headers: &HeaderMap, origin: &str) -> bool {
    let Some(origin) = cccc_core::web_login_grants::normalize_origin(origin) else {
        return false;
    };
    if served_origin(headers).as_deref() == Some(origin.as_str()) {
        return true;
    }
    configured_origins().any(|allowed| allowed == origin)
}

pub fn cookie_csrf_allowed(headers: &HeaderMap) -> bool {
    source_origin(headers).is_some_and(|origin| origin_allowed(headers, &origin))
}

pub fn is_https(headers: &HeaderMap) -> bool {
    served_origin(headers).is_some_and(|origin| origin.starts_with("https://"))
}

fn configured_origins() -> impl Iterator<Item = String> {
    std::env::var("CCCC_WEB_CORS_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|value| {
            let value = value.trim();
            (!value.is_empty() && value != "*")
                .then(|| cccc_core::web_login_grants::normalize_origin(value))
                .flatten()
        })
        .collect::<Vec<_>>()
        .into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers() -> HeaderMap {
        HeaderMap::from_iter([
            (header::HOST, HeaderValue::from_static("cccc.example")),
            (
                header::HeaderName::from_static("x-forwarded-proto"),
                HeaderValue::from_static("https"),
            ),
        ])
    }

    #[test]
    fn cookie_csrf_requires_the_exact_served_origin() {
        let mut same = headers();
        same.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://cccc.example"),
        );
        assert!(cookie_csrf_allowed(&same));

        let mut sibling = headers();
        sibling.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!cookie_csrf_allowed(&sibling));
        assert!(!cookie_csrf_allowed(&headers()));
    }

    #[test]
    fn referer_is_an_allowed_fallback() {
        let mut request = headers();
        request.insert(
            header::REFERER,
            HeaderValue::from_static("https://cccc.example/ui/settings"),
        );
        assert!(cookie_csrf_allowed(&request));
    }

    #[test]
    fn forwarded_host_preserves_the_browser_origin_through_a_loopback_proxy() {
        let mut request = HeaderMap::from_iter([
            (header::HOST, HeaderValue::from_static("127.0.0.1:8848")),
            (
                header::HeaderName::from_static("x-forwarded-host"),
                HeaderValue::from_static("localhost:5555"),
            ),
            (
                header::HeaderName::from_static("x-forwarded-proto"),
                HeaderValue::from_static("http"),
            ),
        ]);
        request.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:5555"),
        );
        assert!(origin_allowed(&request, "http://localhost:5555"));
    }

    #[test]
    fn forwarded_header_is_supported_when_legacy_headers_are_absent() {
        let request = HeaderMap::from_iter([
            (header::HOST, HeaderValue::from_static("127.0.0.1:8848")),
            (
                header::HeaderName::from_static("forwarded"),
                HeaderValue::from_static("for=192.0.2.1;proto=https;host=\"cccc.example\""),
            ),
        ]);
        assert_eq!(
            served_origin(&request).as_deref(),
            Some("https://cccc.example")
        );
    }

    #[test]
    fn forwarded_proto_chain_uses_the_browser_facing_value() {
        let request = HeaderMap::from_iter([
            (
                header::HeaderName::from_static("x-forwarded-host"),
                HeaderValue::from_static("cccc.example, 127.0.0.1:8848"),
            ),
            (
                header::HeaderName::from_static("x-forwarded-proto"),
                HeaderValue::from_static("https, http"),
            ),
        ]);
        assert_eq!(
            served_origin(&request).as_deref(),
            Some("https://cccc.example")
        );
    }
}

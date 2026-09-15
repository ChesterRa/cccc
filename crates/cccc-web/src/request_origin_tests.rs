use super::*;
use axum::http::{HeaderMap, HeaderValue, header};

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
    assert!(origin_allowed_with_proxy(
        &same,
        "https://cccc.example",
        true
    ));

    let mut sibling = headers();
    sibling.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://evil.example"),
    );
    assert!(!origin_allowed_with_proxy(
        &sibling,
        "https://evil.example",
        true
    ));
    assert!(source_origin(&headers()).is_none());
}

#[test]
fn referer_is_an_allowed_fallback() {
    let mut request = headers();
    request.insert(
        header::REFERER,
        HeaderValue::from_static("https://cccc.example/ui/settings"),
    );
    assert!(
        source_origin(&request)
            .is_some_and(|origin| { origin_allowed_with_proxy(&request, &origin, true) })
    );
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
    assert!(origin_allowed_with_proxy(
        &request,
        "http://localhost:5555",
        true
    ));
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
        served_origin_with_proxy(&request, true).as_deref(),
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
        served_origin_with_proxy(&request, true).as_deref(),
        Some("https://cccc.example")
    );
}

#[test]
fn untrusted_forwarded_headers_cannot_replace_the_direct_origin() {
    let request = HeaderMap::from_iter([
        (header::HOST, HeaderValue::from_static("direct.example")),
        (
            header::HeaderName::from_static("x-forwarded-host"),
            HeaderValue::from_static("evil.example"),
        ),
        (
            header::HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static("https"),
        ),
    ]);
    assert_eq!(
        served_origin_with_proxy(&request, false).as_deref(),
        Some("http://direct.example")
    );
}

#[test]
fn supervised_proxy_trust_requires_a_loopback_binding() {
    assert!(proxy_headers_trusted_for(true, false, Some("127.0.0.1")));
    assert!(!proxy_headers_trusted_for(true, false, Some("0.0.0.0")));
    assert!(!proxy_headers_trusted_for(true, false, None));
    assert!(proxy_headers_trusted_for(false, true, Some("0.0.0.0")));
}

#[test]
fn tls_terminating_proxies_keep_same_host_browser_requests_authorized() {
    // Chrome sends no Sec-Fetch-* header on a WebSocket handshake, and an
    // untrusted proxy hides the browser-facing scheme, so the served origin
    // reads back as http:// for an https:// page.
    let mut request =
        HeaderMap::from_iter([(header::HOST, HeaderValue::from_static("cccc.tae.example"))]);
    request.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://cccc.tae.example"),
    );
    assert_eq!(
        served_origin_with_proxy(&request, false).as_deref(),
        Some("http://cccc.tae.example")
    );
    assert!(cookie_csrf_allowed_with_proxy(&request, false));
}

#[test]
fn scheme_agnostic_matching_still_rejects_other_hosts_and_ports() {
    let mut sibling =
        HeaderMap::from_iter([(header::HOST, HeaderValue::from_static("cccc.tae.example"))]);
    sibling.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://evil.example"),
    );
    assert!(!cookie_csrf_allowed_with_proxy(&sibling, false));

    let mut other_port = HeaderMap::from_iter([(
        header::HOST,
        HeaderValue::from_static("cccc.tae.example:8848"),
    )]);
    other_port.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://cccc.tae.example:9999"),
    );
    assert!(!cookie_csrf_allowed_with_proxy(&other_port, false));

    // A subdomain is a different host, not a same-site relaxation.
    let mut subdomain =
        HeaderMap::from_iter([(header::HOST, HeaderValue::from_static("cccc.tae.example"))]);
    subdomain.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://evil.cccc.tae.example"),
    );
    assert!(!cookie_csrf_allowed_with_proxy(&subdomain, false));
}

#[test]
fn default_ports_normalize_across_schemes() {
    let mut request = HeaderMap::from_iter([(
        header::HOST,
        HeaderValue::from_static("cccc.tae.example:443"),
    )]);
    request.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://cccc.tae.example"),
    );
    assert!(cookie_csrf_allowed_with_proxy(&request, false));
}

#[test]
fn explicit_ports_are_not_interchangeable_with_other_schemes_defaults() {
    for (host, origin) in [
        ("cccc.example:443", "https://cccc.example:80"),
        ("cccc.example:80", "https://cccc.example"),
        ("cccc.example", "https://cccc.example:80"),
    ] {
        let mut request = HeaderMap::new();
        request.insert(
            header::HOST,
            HeaderValue::from_str(host).expect("Host fixture"),
        );
        request.insert(
            header::ORIGIN,
            HeaderValue::from_str(origin).expect("Origin fixture"),
        );
        assert!(
            !cookie_csrf_allowed_with_proxy(&request, false),
            "{host} accepted {origin}"
        );
    }
}

#[test]
fn a_trusted_proxy_scheme_is_not_discarded() {
    let request = headers();
    assert!(!origin_allowed_with_proxy(
        &request,
        "http://cccc.example",
        true
    ));
}

#[test]
fn ipv6_authorities_keep_explicit_ports_exact() {
    let request = HeaderMap::from_iter([(header::HOST, HeaderValue::from_static("[::1]:8848"))]);
    assert!(origin_allowed_with_proxy(
        &request,
        "https://[::1]:8848",
        false
    ));
    assert!(!origin_allowed_with_proxy(
        &request,
        "https://[::1]:8849",
        false
    ));
}

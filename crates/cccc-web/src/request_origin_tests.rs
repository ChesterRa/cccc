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
fn any_origin_switch_is_opt_in() {
    if let Ok(expected) = std::env::var("CCCC_TEST_ANY_ORIGIN_CHILD") {
        assert_eq!(allow_any_origin(), expected == "true");
        return;
    }
    // 与现有 socket 测试一样，只为子进程设置环境，避免并行测试修改全局状态。
    for (value, expected) in [(None, false), (Some("1"), true), (Some("0"), false)] {
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"));
        child
            .args([
                "--exact",
                "request_origin::tests::any_origin_switch_is_opt_in",
                "--nocapture",
            ])
            .env("CCCC_TEST_ANY_ORIGIN_CHILD", expected.to_string())
            .env_remove("CCCC_WEB_ALLOW_ANY_ORIGIN");
        if let Some(value) = value {
            child.env("CCCC_WEB_ALLOW_ANY_ORIGIN", value);
        }
        let output = child.output().expect("isolated test child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

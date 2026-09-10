use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore};
use std::net::SocketAddr;
use tower::ServiceExt;

// Environment configuration belongs to a process, not to concurrent tests.
fn isolated(case: &str, any_origin: bool, origins: &str) {
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "configured_origin_worker", "--nocapture"])
        .env("CCCC_TEST_CORS_CASE", case)
        .env(
            "CCCC_WEB_ALLOW_ANY_ORIGIN",
            if any_origin { "1" } else { "0" },
        )
        .env("CCCC_WEB_CORS_ORIGINS", origins)
        .env_remove("CCCC_WEB_TRUST_PROXY_HEADERS")
        .output()
        .expect("isolated test");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn wildcard_cors_supports_explicit_bearer_requests() {
    isolated("bearer", true, "");
}

#[test]
fn wildcard_cors_does_not_authorize_cookie_writes_or_websockets() {
    isolated("cookie", true, "");
}

#[test]
fn websocket_origins_follow_the_authentication_source() {
    isolated("websocket", false, "https://client.example");
}

#[test]
fn wildcard_cors_does_not_expose_passwordless_local_reads() {
    isolated("local", true, "");
}

#[test]
fn exact_cors_origins_support_credentials() {
    isolated("exact", false, "https://client.example");
}

#[test]
fn cors_is_opt_in() {
    isolated("default", false, "");
}

#[tokio::test]
async fn configured_origin_worker() {
    let Ok(case) = std::env::var("CCCC_TEST_CORS_CASE") else {
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("test-admin", Vec::new(), true, None)
        .expect("token");
    let app = cccc_web::app(home);
    let origin = "https://client.example";
    if case == "websocket" {
        for bearer in [false, true] {
            for source in [
                None,
                Some("null"),
                Some("https://evil.example"),
                Some(origin),
            ] {
                let mut request = Request::get("/api/v1/access-tokens")
                    .header(header::HOST, "internal-proxy:8848")
                    .header(header::UPGRADE, "WebSocket");
                if let Some(source) = source {
                    request = request.header(header::ORIGIN, source);
                }
                request = if bearer {
                    request.header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                } else {
                    request.header(header::COOKIE, format!("cccc_access_token={}", token.token))
                };
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).expect("request"))
                    .await
                    .expect("websocket authorization");
                assert_eq!(
                    response.status(),
                    if bearer || source == Some(origin) {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                    "bearer={bearer}, origin={source:?}"
                );
            }
        }
        return;
    }
    if matches!(case.as_str(), "bearer" | "exact" | "default") {
        let response = app
            .clone()
            .oneshot(
                Request::options("/api/v1/access-tokens")
                    .header(header::HOST, "cccc.example")
                    .header(header::ORIGIN, origin)
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type",
                    )
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("preflight");
        if case == "default" {
            assert!(
                !response
                    .headers()
                    .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            );
            return;
        }
        assert!(response.status().is_success());
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            if case == "exact" { origin } else { "*" }
        );
        // Authorization is a non-wildcard request-header name in browser CORS.
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
            "authorization,content-type"
        );
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/access-tokens")
                    .header(header::HOST, "cccc.example")
                    .header(header::ORIGIN, origin)
                    .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("bearer request");
        assert_eq!(response.status(), StatusCode::OK);
        if case == "bearer" {
            assert!(
                !response
                    .headers()
                    .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            );
            return;
        }
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS],
            "true"
        );
    }
    if matches!(case.as_str(), "cookie" | "exact") {
        for websocket in [false, true] {
            let mut request = if websocket {
                Request::get("/api/v1/access-tokens")
            } else {
                Request::post("/api/v1/web_access/logout")
            }
            .header(header::HOST, "cccc.example")
            .header(header::ORIGIN, origin)
            .header(header::COOKIE, format!("cccc_access_token={}", token.token));
            if websocket {
                request = request.header(header::UPGRADE, "websocket");
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).expect("request"))
                .await
                .expect("cookie request");
            assert_eq!(
                response.status(),
                if case == "exact" {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
                "websocket={websocket}"
            );
        }
    }
    if case == "cookie" {
        // Removing the Origin gate must not grant an unauthenticated request access.
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/v1/access-tokens")
                    .header(header::HOST, "cccc.example")
                    .header(header::ORIGIN, origin)
                    .header(header::UPGRADE, "websocket")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("unauthenticated request");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    if case == "local" {
        for source in [
            None,
            Some("http://127.0.0.1:8848"),
            Some(origin),
            Some("null"),
        ] {
            let mut request = Request::get("/api/v1/access-tokens")
                .header(header::HOST, "127.0.0.1:8848")
                .extension(ConnectInfo(
                    "127.0.0.1:42000".parse::<SocketAddr>().expect("peer"),
                ));
            if let Some(source) = source {
                request = request.header(header::ORIGIN, source);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).expect("request"))
                .await
                .expect("local request");
            assert_eq!(
                response.status(),
                if source.is_none() || source == Some("http://127.0.0.1:8848") {
                    StatusCode::OK
                } else {
                    StatusCode::UNAUTHORIZED
                },
                "source={source:?}"
            );
        }
    }
}

use super::*;
use axum::http::Request;
use tower::ServiceExt;

#[tokio::test]
async fn spa_fallback_uses_index_html_content_type() {
    for path in ["/ui/capabilities", "/ui/capabilities/"] {
        let response = static_asset(path.parse().expect("URI")).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&header::HeaderValue::from_static("text/html")),
            "unexpected content type for {path}"
        );
    }
}

#[tokio::test]
async fn static_assets_negotiate_gzip_compression() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let response = app_with_mode(home, WebMode::Normal)
        .oneshot(
            Request::builder()
                .uri("/ui/logo.svg")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_ENCODING),
        Some(&header::HeaderValue::from_static("gzip")),
    );
}

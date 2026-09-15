mod auth_support;
#[allow(dead_code)]
mod workspace_support;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, Scope, access_tokens::AccessTokenStore, group_scope};
use tower::ServiceExt;
use workspace_support::{Fixture, fixture};

fn content_url(fixture: &Fixture, path: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("scope_key", "scope_repo")
        .append_pair(
            "scope_url",
            &fixture
                .repo
                .canonicalize()
                .expect("scope")
                .to_string_lossy(),
        )
        .append_pair("path", path)
        .finish();
    format!(
        "/api/v1/groups/{}/workspace/content?{query}",
        fixture.group_id
    )
}

async fn get(
    app: &axum::Router,
    request: axum::http::request::Builder,
) -> axum::response::Response {
    app.clone()
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response")
}

#[tokio::test]
async fn streams_large_media_and_seeks_without_the_text_preview_limit() {
    let fixture = fixture();
    let data: Vec<u8> = (0..2_100_123).map(|i| (i % 251) as u8).collect();
    std::fs::write(fixture.repo.join("clip.mp4"), &data).expect("video");
    let app = auth_support::authenticated_app(fixture.home.clone());
    let url = content_url(&fixture, "clip.mp4");
    for (range, start, end) in [
        ("bytes=10-99", 10, 99),
        ("bytes=2100000-", 2_100_000, data.len() - 1),
        ("bytes=-123", data.len() - 123, data.len() - 1),
        ("bytes=2100100-9999999", 2_100_100, data.len() - 1),
    ] {
        let response = get(&app, Request::get(&url).header(header::RANGE, range)).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            (end - start + 1).to_string()
        );
        assert_eq!(
            response.headers()[header::CONTENT_RANGE],
            format!("bytes {start}-{end}/{}", data.len())
        );
        assert_eq!(
            to_bytes(response.into_body(), data.len())
                .await
                .expect("body"),
            &data[start..=end]
        );
    }
    let response = get(&app, Request::head(&url)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_LENGTH],
        data.len().to_string()
    );
    assert!(
        to_bytes(response.into_body(), 0)
            .await
            .expect("head")
            .is_empty()
    );
    let response = get(&app, Request::get(&url)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), data.len())
            .await
            .expect("body"),
        data
    );
}

#[tokio::test]
async fn ranges_reject_unsatisfiable_requests_and_if_range_restarts_changed_files() {
    let fixture = fixture();
    std::fs::write(fixture.repo.join("clip.mp4"), b"0123456789").expect("video");
    let app = auth_support::authenticated_app(fixture.home.clone());
    let url = content_url(&fixture, "clip.mp4");
    for range in ["bytes=10-", "bytes=8-2", "bytes=0-1,5-6"] {
        let response = get(&app, Request::get(&url).header(header::RANGE, range)).await;
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{range}"
        );
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    }
    let response = get(
        &app,
        Request::get(&url)
            .header(header::RANGE, "bytes=2-3")
            .header(header::IF_RANGE, "Thu, 01 Jan 1970 00:00:00 GMT"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 10).await.expect("body"),
        "0123456789"
    );
    std::fs::write(fixture.repo.join("empty.mp4"), []).expect("empty");
    let response = get(&app, Request::get(content_url(&fixture, "empty.mp4"))).await;
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
}

#[tokio::test]
async fn keeps_svg_inert_and_downloads_active_documents_with_safe_names() {
    let fixture = fixture();
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' onload='alert(1)'/>";
    std::fs::write(fixture.repo.join("drawing.svg"), svg).expect("svg");
    std::fs::write(
        fixture.repo.join("report.html"),
        "<script>alert(1)</script>",
    )
    .expect("html");
    let app = auth_support::authenticated_app(fixture.home.clone());
    let response = get(&app, Request::get(content_url(&fixture, "drawing.svg"))).await;
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    let policies = response
        .headers()
        .get_all(header::CONTENT_SECURITY_POLICY)
        .iter()
        .map(|value| value.to_str().expect("policy"))
        .collect::<Vec<_>>();
    assert!(
        policies
            .iter()
            .any(|policy| policy.contains("sandbox; default-src 'none'"))
    );
    assert!(
        policies
            .iter()
            .any(|policy| policy.contains("frame-ancestors 'self'"))
    );
    for path in ["drawing.svg", "report.html"] {
        let response = get(
            &app,
            Request::get(format!("{}&download=true", content_url(&fixture, path))),
        )
        .await;
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/octet-stream"
        );
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .expect("disposition")
                .starts_with("attachment;")
        );
    }
    let response = get(&app, Request::get(content_url(&fixture, "report.html"))).await;
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert!(response.headers().contains_key(header::CONTENT_DISPOSITION));
}

#[tokio::test]
async fn enforces_group_permissions_exhibit_and_connect_frame_on_every_request() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let tokens = AccessTokenStore::new(fixture.home.clone()).expect("tokens");
    tokens
        .create(
            "limited",
            vec!["g_unrelated".into()],
            false,
            Some("workspace-limited-fixture"),
        )
        .expect("limited token");
    let url = content_url(&fixture, "src/lib.rs");
    let response = get(
        &app,
        Request::get(&url).header(header::AUTHORIZATION, "Bearer workspace-limited-fixture"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = get(
        &app,
        Request::get(&url).header(header::AUTHORIZATION, "Bearer invalid-fixture"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = get(
        &app,
        Request::get(format!("{url}&connect_frame=revoked-frame")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let app =
        auth_support::authenticated_app_with_mode(fixture.home.clone(), cccc_web::WebMode::Exhibit);
    assert_eq!(
        get(&app, Request::get(&url)).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn native_media_uses_the_scoped_browser_cookie_and_rechecks_revocation() {
    let fixture = fixture();
    let tokens = AccessTokenStore::new(fixture.home.clone()).expect("tokens");
    tokens
        .create("admin", Vec::new(), true, Some("media-admin-fixture"))
        .expect("admin");
    let viewer = tokens
        .create(
            "viewer",
            vec![fixture.group_id.clone()],
            false,
            Some("media-viewer-fixture"),
        )
        .expect("viewer");
    let app = cccc_web::app(fixture.home.clone());
    let url = content_url(&fixture, "src/lib.rs");
    let authenticated = get(
        &app,
        Request::get("/api/v1/web_access/session")
            .header(header::AUTHORIZATION, "Bearer media-viewer-fixture"),
    )
    .await;
    assert_eq!(authenticated.status(), StatusCode::OK);
    let cookie = authenticated.headers()[header::SET_COOKIE]
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("pair")
        .to_owned();
    let native = get(
        &app,
        Request::get(&url)
            .header(header::COOKIE, &cookie)
            .header(header::RANGE, "bytes=0-2"),
    )
    .await;
    assert_eq!(native.status(), StatusCode::PARTIAL_CONTENT);
    tokens.delete(&viewer.token_id()).expect("revoke");
    let revoked = get(
        &app,
        Request::get(&url)
            .header(header::COOKIE, cookie)
            .header(header::RANGE, "bytes=3-5"),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_stale_scope_and_path_escape_including_later_range_requests() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    for path in ["../secret.txt", "/etc/passwd", "src", "missing.png"] {
        assert!(
            get(&app, Request::get(content_url(&fixture, path)))
                .await
                .status()
                .is_client_error()
        );
    }
    let url = content_url(&fixture, "src/lib.rs");
    assert_eq!(get(&app, Request::get(&url)).await.status(), StatusCode::OK);
    let other = fixture.repo.join("other");
    std::fs::create_dir_all(other.join("src")).expect("other");
    std::fs::write(other.join("src/lib.rs"), "must not leak").expect("other file");
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    group_scope::attach(
        &store,
        &fixture.group_id,
        Scope {
            scope_key: "scope_other".into(),
            url: other.to_string_lossy().into_owned(),
            label: "other".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let response = get(&app, Request::get(&url).header(header::RANGE, "bytes=0-3")).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        get(
            &app,
            Request::get(url.replace("scope_key=scope_repo", "scope_key="))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[cfg(unix)]
#[tokio::test]
async fn confines_symlinks_and_preserves_literal_posix_filenames() {
    let fixture = fixture();
    std::os::unix::fs::symlink("/etc/passwd", fixture.repo.join("escape.png")).expect("symlink");
    std::fs::write(fixture.repo.join("literal\\name.png"), b"literal").expect("literal");
    std::os::unix::fs::symlink("literal\\name.png", fixture.repo.join("alias.png")).expect("alias");
    let app = auth_support::authenticated_app(fixture.home.clone());
    assert_eq!(
        get(&app, Request::get(content_url(&fixture, "escape.png")))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    for path in ["literal\\name.png", "alias.png"] {
        let response = get(&app, Request::get(content_url(&fixture, path))).await;
        assert_eq!(
            to_bytes(response.into_body(), 10).await.expect("body"),
            "literal"
        );
    }
}

#[tokio::test]
async fn streams_pdf_and_audio_with_native_mime_and_explicit_download() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    for (name, mime, data) in [
        (
            "report.pdf",
            "application/pdf",
            b"%PDF-1.7\nfixture".as_slice(),
        ),
        ("sound.wav", "audio/wav", b"RIFFfixture".as_slice()),
    ] {
        std::fs::write(fixture.repo.join(name), data).expect("write");
        let response = get(
            &app,
            Request::get(content_url(&fixture, name)).header(header::RANGE, "bytes=0-3"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_TYPE], mime);
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .expect("header")
                .starts_with("inline;")
        );
        assert_eq!(
            to_bytes(response.into_body(), 4).await.expect("bytes"),
            &data[..4]
        );
        let response = get(
            &app,
            Request::get(format!("{}&download=true", content_url(&fixture, name))),
        )
        .await;
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/octet-stream"
        );
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .expect("header")
                .starts_with("attachment;")
        );
        assert_eq!(
            to_bytes(response.into_body(), data.len())
                .await
                .expect("download"),
            data
        );
    }
}

use super::*;

#[tokio::test]
async fn building_the_standalone_web_app_installs_a_tls_provider() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");

    let _router = app(home);

    assert!(
        rustls::crypto::CryptoProvider::get_default().is_some(),
        "the Web crate must initialize TLS without relying on the CLI binary"
    );
}

#[tokio::test]
async fn windows_reserved_port_retries_with_zero_and_returns_the_effective_listener() {
    let attempts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = std::sync::Arc::clone(&attempts);
    let listener =
        web_listener::bind_web_listener_with("127.0.0.1", 8848, true, move |host, port| {
            let observed = std::sync::Arc::clone(&observed);
            async move {
                observed
                    .lock()
                    .expect("attempts")
                    .push((host.clone(), port));
                if port == 8848 {
                    Err(std::io::Error::from_raw_os_error(10013))
                } else {
                    tokio::net::TcpListener::bind((host, port)).await
                }
            }
        })
        .await
        .expect("fallback listener");

    assert_eq!(
        *attempts.lock().expect("attempts"),
        [("127.0.0.1".to_owned(), 8848), ("127.0.0.1".to_owned(), 0)]
    );
    assert!(listener.local_addr().expect("effective address").port() > 0);
}
use futures_util::StreamExt;
use tower::ServiceExt;

#[tokio::test]
async fn explicit_shutdown_stops_web_server() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        serve_until(home, "127.0.0.1", 0, async {}),
    )
    .await
    .expect("Web shutdown timeout")
    .expect("Web result");
    assert!(result.port() > 0);
}

#[test]
fn remote_listener_requires_an_administrator_access_token() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_err());
    assert!(ensure_listener_auth(&home, "127.0.0.1:8848".parse().expect("address")).is_ok());
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("scoped", vec!["g_test".into()], false, None)
        .expect("scoped token");
    assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_err());
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    assert!(ensure_listener_auth(&home, "0.0.0.0:8848".parse().expect("address")).is_ok());
}

#[tokio::test]
async fn scoped_tokens_cannot_access_global_codex_voice_authority() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("groups")
        .create("Voice scope", "")
        .expect("group");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("scoped", vec![group.group_id.clone()], false, None)
        .expect("scoped token");
    let router = app_with_mode(home, WebMode::Normal);

    for request in [
        Request::builder()
            .uri("/api/v1/codex_voice/calls/active")
            .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
            .body(Body::empty())
            .expect("active request"),
        Request::builder()
            .method("POST")
            .uri("/api/v1/codex_voice/calls")
            .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
            .body(Body::empty())
            .expect("start request"),
    ] {
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn shutdown_closes_active_sse_response() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let (shutdown, _) = broadcast::channel(1);
    let response = app_with_shutdown(
        home,
        shutdown.clone(),
        WebMode::Normal,
        None,
        LiveBinding::from_env(),
        new_web_runtime_id(),
    )
    .0
    .oneshot(
        axum::http::Request::builder()
            .uri("/api/v1/events/stream")
            .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("SSE response");
    let mut body = response.into_body().into_data_stream();
    tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
        .await
        .expect("connected event timeout")
        .expect("connected event missing")
        .expect("connected event");
    shutdown.send(()).expect("active SSE subscriber");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
            .await
            .expect("SSE shutdown timeout")
            .is_none()
    );
}

#[tokio::test]
async fn shutdown_closes_headless_sse_response() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.create("headless shutdown", "").expect("group");
    let events = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless/events.jsonl");
    std::fs::create_dir_all(events.parent().expect("events parent")).expect("headless dir");
    std::fs::write(&events, "").expect("events file");
    let (shutdown, _) = broadcast::channel(1);
    let response = app_with_shutdown(
        home,
        shutdown.clone(),
        WebMode::Normal,
        None,
        LiveBinding::from_env(),
        new_web_runtime_id(),
    )
    .0
    .oneshot(
        axum::http::Request::builder()
            .uri(format!(
                "/api/v1/groups/{}/headless/stream?replay=false",
                group.group_id
            ))
            .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("headless SSE response");
    let mut body = response.into_body().into_data_stream();
    shutdown.send(()).expect("active headless SSE subscriber");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
            .await
            .expect("headless SSE shutdown timeout")
            .is_none()
    );
}

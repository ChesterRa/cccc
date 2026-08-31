use super::*;
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

mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use tower::ServiceExt;

#[tokio::test]
async fn exhibit_mode_rejects_writes_with_stable_error() {
    let (_temp, home) = home();
    let response = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit)
        .oneshot(
            Request::post("/api/v1/groups")
                .body(Body::from(r#"{"title":"blocked"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["error"]["code"], "read_only");
}

#[tokio::test]
async fn normal_mode_does_not_apply_read_only_guard() {
    let (_temp, home) = home();
    let response = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Normal)
        .oneshot(
            Request::post("/missing")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn exhibit_mode_rejects_mutating_get_and_websocket_routes() {
    let (_temp, home) = home();
    let app = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit);
    for path in [
        "/api/v1/registry/reconcile",
        "/nomcp/s/session-1/send",
        "/api/group-bridge/session/ws",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload["error"]["code"], "read_only", "{path}");
    }
}

#[tokio::test]
async fn exhibit_mode_rejects_filesystem_reads_like_legacy_web() {
    let (_temp, home) = home();
    let app = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit);
    for path in ["/api/v1/fs/recent", "/api/v1/fs/list?path=~"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        let payload: Value = serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes(),
        )
        .expect("json");
        assert_eq!(payload["error"]["code"], "read_only", "{path}");
    }
}

#[tokio::test]
async fn exhibit_mode_rejects_directory_creation() {
    let (temp, home) = home();
    let response = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit)
        .oneshot(
            Request::post("/api/v1/fs/directory")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"parent": temp.path(), "name": "blocked"}).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!temp.path().join("blocked").exists());
}

#[tokio::test]
async fn exhibit_ping_matches_python_contract() {
    let (_temp, home) = home();
    let token = cccc_core::access_tokens::AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response =
        auth_support::authenticated_app_with_mode(home.clone(), cccc_web::WebMode::Exhibit)
            .oneshot(
                Request::get("/api/v1/ping?include_home=true")
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format!("Bearer {}", token.token),
                    )
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["result"]["web"]["mode"], "exhibit");
    assert_eq!(payload["result"]["web"]["read_only"], true);
    assert_eq!(
        payload["result"]["home"],
        home.root().to_string_lossy().as_ref()
    );
    assert_eq!(payload["result"]["daemon"]["implementation"], "rust");

    shutdown_daemon(&home).await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[tokio::test]
async fn exhibit_browser_surface_socket_reports_read_only_error() {
    let (_temp, home) = home();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit),
        )
        .await
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/api/v1/groups/g_test/presentation/browser_surface/ws?slot=main"
    ))
    .await
    .expect("connect");
    let message = socket
        .next()
        .await
        .expect("message")
        .expect("websocket message");
    let payload: Value = serde_json::from_str(message.to_text().expect("text")).expect("json");
    assert_eq!(payload["error"]["code"], "read_only_browser_surface");
    server.abort();
}

async fn wait_for_daemon(home: &HomeLayout) {
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Map::new(),
            })
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

async fn shutdown_daemon(home: &HomeLayout) {
    cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await
        .expect("shutdown daemon");
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    (temp, home)
}

use super::*;
use axum::extract::ConnectInfo;
use cccc_contracts::DaemonRequest;
use serde_json::{Map, json};
use std::net::SocketAddr;

fn peer(address: &str) -> ConnectInfo<SocketAddr> {
    ConnectInfo(address.parse().expect("peer address"))
}

fn local_get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, "127.0.0.1:8848")
        .extension(peer("127.0.0.1:42000"))
        .body(Body::empty())
        .expect("request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn direct_loopback_web_is_passwordless_without_creating_a_token() {
    let (_temp, home) = home();
    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::get("/api/v1/access-tokens")
                .header(header::HOST, "127.0.0.1:8848")
                .extension(peer("127.0.0.1:42000"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        AccessTokenStore::new(home)
            .expect("store")
            .list()
            .expect("tokens")
            .is_empty()
    );
}

#[tokio::test]
async fn configured_tokens_do_not_hide_the_local_admin_session_projection() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("configured-admin", Vec::new(), true, None)
        .expect("admin token");
    let app = cccc_web::app(home);

    let session = app
        .clone()
        .oneshot(local_get("/api/v1/web_access/session"))
        .await
        .expect("session response");
    assert_eq!(session.status(), StatusCode::OK);
    let session = response_json(session).await;
    let projection = &session["result"]["web_access_session"];
    assert_eq!(projection["login_active"], true);
    assert_eq!(projection["current_browser_signed_in"], true);
    assert_eq!(projection["access_token_count"], 1);
    assert_eq!(projection["can_access_global_settings"], true);
    assert_eq!(projection["user_id"], "local");

    let ready = app
        .oneshot(local_get(
            "/api/v1/ready?challenge=local-runtime-proof-challenge",
        ))
        .await
        .expect("ready response");
    assert_eq!(ready.status(), StatusCode::OK);
    let ready = response_json(ready).await;
    assert!(ready["result"]["runtime_id"].as_str().is_some());
    assert!(ready["result"]["proof"].as_str().is_some());
}

#[tokio::test]
async fn configured_tokens_do_not_hide_the_local_admin_ping_projection() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("configured-admin", Vec::new(), true, None)
        .expect("admin token");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = cccc_web::app(home.clone())
        .oneshot(local_get("/api/v1/ping?include_home=true"))
        .await
        .expect("ping response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["result"]["web"]["read_only"], false);
    assert_eq!(
        payload["result"]["home"],
        json!(home.root().to_string_lossy())
    );

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[tokio::test]
async fn remote_peer_cannot_spoof_a_loopback_host_for_passwordless_access() {
    let (_temp, home) = home();
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/access-tokens")
                .header(header::HOST, "127.0.0.1:8848")
                .extension(peer("192.0.2.10:42000"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn passwordless_local_write_requires_the_exact_loopback_origin() {
    let (_temp, home) = home();
    let app = cccc_web::app(home);
    let allowed = app
        .clone()
        .oneshot(
            Request::post("/api/v1/web_access/logout")
                .header(header::HOST, "localhost:8848")
                .header(header::ORIGIN, "http://localhost:8848")
                .extension(peer("[::1]:42000"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(allowed.status(), StatusCode::OK);

    let missing_origin = app
        .oneshot(
            Request::post("/api/v1/web_access/logout")
                .header(header::HOST, "localhost:8848")
                .extension(peer("127.0.0.1:42000"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(missing_origin.status(), StatusCode::UNAUTHORIZED);
}

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("daemon address was not created");
}

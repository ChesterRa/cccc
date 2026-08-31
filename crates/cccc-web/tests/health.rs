use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use tower::ServiceExt;

#[tokio::test]
async fn anonymous_health_fails_when_the_daemon_is_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload = response_json(response).await;
    assert_eq!(payload["error"]["code"], "daemon_unavailable");

    let response = cccc_web::app(HomeLayout::from_path(temp.path().join("home")).expect("home"))
        .oneshot(
            Request::get("/api/v1/ping")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn anonymous_health_checks_the_daemon_without_disclosing_details() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;

    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::get("/api/v1/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["result"]["status"], "ok");
    assert!(payload["result"]["pid"].is_null());

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
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

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("daemon address was not created");
}

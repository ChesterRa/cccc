use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn runtime_endpoint_returns_frontend_availability_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/runtimes")
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
    let runtimes = payload["result"]["runtimes"].as_array().expect("runtimes");
    assert!(!runtimes.is_empty());
    assert!(runtimes.iter().any(|runtime| {
        runtime["name"] == "custom"
            && runtime["display_name"] == "Custom"
            && runtime["available"] == true
    }));
    assert!(runtimes.iter().any(|runtime| {
        runtime["name"] == "cline"
            && runtime["display_name"] == "Cline CLI"
            && runtime["recommended_command"] == "cline --tui --auto-approve true"
    }));
}

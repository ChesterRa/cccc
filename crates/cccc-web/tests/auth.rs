use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn first_admin_token_bootstraps_login_cookie() {
    let (_temp, home) = home();
    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/access-tokens")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"user_id":"admin","is_admin":true}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("cccc_access_token=acc_"))
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(
        payload["result"]["access_token"]["token"]
            .as_str()
            .is_some_and(|token| token.starts_with("acc_"))
    );
}

#[tokio::test]
async fn configured_tokens_reject_anonymous_api_requests() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn scoped_token_cannot_open_another_group() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups/g_denied/actors")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    (temp, home)
}

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

#[tokio::test]
async fn legacy_flat_token_document_keeps_authentication_enabled() {
    let (_temp, home) = home();
    std::fs::write(
        home.root().join("access_tokens.yaml"),
        concat!(
            "legacy-flat-token:\n",
            "  user_id: legacy-user\n",
            "  allowed_groups: []\n",
            "  is_admin: true\n",
            "  created_at: '2026-01-01T00:00:00Z'\n",
            "  updated_at: '2026-01-01T00:00:00Z'\n",
        ),
    )
    .expect("fixture");

    let anonymous = cccc_web::app(home.clone())
        .oneshot(
            Request::get("/api/v1/groups")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let session = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session?token=legacy-flat-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = session
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["web_access_session"]["current_browser_signed_in"],
        true
    );
}

#[tokio::test]
async fn encoded_custom_token_authenticates_event_source_queries() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, Some("token;+ 含"))
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session?token=token%3B%2B%20%E5%90%AB")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["web_access_session"]["current_browser_signed_in"],
        true
    );
}

#[tokio::test]
async fn cannot_delete_the_last_admin_while_scoped_tokens_remain() {
    let (_temp, home) = home();
    let store = AccessTokenStore::new(home.clone()).expect("store");
    let admin = store
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    store
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("member");
    let response = cccc_web::app(home)
        .oneshot(
            Request::delete(format!("/api/v1/access-tokens/{}", admin.token_id()))
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    (temp, home)
}

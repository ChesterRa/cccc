mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(app: axum::Router, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut request =
        Request::post("/api/v1/fs/directory").header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&body).expect("json"))
}

#[tokio::test]
async fn create_directory_requires_admin_and_creates_one_child() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = AccessTokenStore::new(home.clone()).expect("tokens");
    let admin = store
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let member = store
        .create("member", Vec::new(), false, None)
        .expect("member");
    let parent = temp.path().join("projects");
    std::fs::create_dir(&parent).expect("parent");
    let body = json!({"parent": parent, "name": "demo"});
    let app = auth_support::authenticated_app(home);

    let (status, _) = post(app.clone(), Some(&member.token), body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, payload) = post(app.clone(), Some(&admin.token), body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["result"]["path"],
        parent
            .join("demo")
            .canonicalize()
            .expect("created directory")
            .to_string_lossy()
            .as_ref()
    );
    assert!(parent.join("demo").is_dir());

    let (status, payload) = post(app, Some(&admin.token), body).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(payload["error"]["code"], "ALREADY_EXISTS");
}

#[tokio::test]
async fn create_directory_rejects_nested_names() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let (status, payload) = post(
        auth_support::authenticated_app(home),
        None,
        json!({"parent": temp.path(), "name": "nested/path"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["error"]["code"], "INVALID_NAME");
    assert!(!temp.path().join("nested").exists());
}

#[tokio::test]
async fn create_directory_rejects_implicit_process_cwd() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let app = auth_support::authenticated_app(home);
    for parent in ["", ".", "relative"] {
        let (status, payload) = post(
            app.clone(),
            None,
            json!({"parent": parent, "name": "must-not-be-created"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{parent:?}: {payload}");
        assert_eq!(payload["error"]["code"], "INVALID_PARENT");
    }
}

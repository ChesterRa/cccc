use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn get(app: axum::Router, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::get(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
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
async fn filesystem_routes_require_admin_and_return_frontend_shapes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = AccessTokenStore::new(home.clone()).expect("tokens");
    let admin = store
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let member = store
        .create("member", vec!["g_one".into()], false, None)
        .expect("member");
    let empty = temp.path().join("empty");
    std::fs::create_dir(&empty).expect("empty dir");
    let url = format!(
        "/api/v1/fs/list?path={}",
        utf8_percent_encode(&empty.to_string_lossy(), NON_ALPHANUMERIC)
    );
    let app = cccc_web::app(home);

    let (status, _) = get(app.clone(), &url, Some(&member.token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, payload) = get(app, &url, Some(&admin.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["result"]["path"].as_str(),
        Some(
            empty
                .canonicalize()
                .expect("canonical")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        payload["result"]["parent"].as_str(),
        Some(
            empty
                .parent()
                .expect("parent")
                .canonicalize()
                .expect("canonical parent")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(payload["result"]["items"], serde_json::json!([]));
    assert_eq!(payload["result"]["readable"], true);
}

#[tokio::test]
async fn filesystem_list_expands_home_and_reports_path_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let app = cccc_web::app(home);

    let (status, payload) = get(app.clone(), "/api/v1/fs/list?path=~", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        payload["result"]["path"]
            .as_str()
            .is_some_and(|path| path.starts_with('/'))
    );
    assert!(payload["result"]["parent"].is_string() || payload["result"]["parent"].is_null());
    assert!(payload["result"]["items"].is_array());

    let missing = temp.path().join("missing");
    let url = format!(
        "/api/v1/fs/list?path={}",
        utf8_percent_encode(&missing.to_string_lossy(), NON_ALPHANUMERIC)
    );
    let (status, payload) = get(app.clone(), &url, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(payload["error"]["code"], "NOT_FOUND");

    let file = temp.path().join("file.txt");
    std::fs::write(&file, "x").expect("file");
    let url = format!(
        "/api/v1/fs/list?path={}",
        utf8_percent_encode(&file.to_string_lossy(), NON_ALPHANUMERIC)
    );
    let (status, payload) = get(app, &url, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["error"]["code"], "NOT_DIR");
}

#[tokio::test]
async fn filesystem_recent_returns_existing_directory_suggestions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let (status, payload) = get(cccc_web::app(home), "/api/v1/fs/recent", None).await;
    assert_eq!(status, StatusCode::OK);
    let suggestions = payload["result"]["suggestions"]
        .as_array()
        .expect("suggestions");
    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().all(|item| {
        item["name"].as_str().is_some_and(|value| !value.is_empty())
            && item["path"]
                .as_str()
                .is_some_and(|path| std::path::Path::new(path).is_dir())
    }));
}

#[tokio::test]
async fn create_group_rejects_present_but_invalid_path_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let app = cccc_web::app(home);
    for path in [serde_json::Value::Null, json!(42), json!("  ")] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/groups")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"title":"demo","path":path}).to_string()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let payload: Value = serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes(),
        )
        .expect("json");
        assert_eq!(payload["error"]["code"], "invalid_path", "{path}");
    }
}

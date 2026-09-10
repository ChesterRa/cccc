mod auth_support;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({})),
    )
}

#[tokio::test]
async fn credentials_are_private_preserved_on_blank_updates_and_explicitly_clearable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let app = auth_support::authenticated_app(home.clone());
    let path = "/api/v1/voice/asr/providers/bailian";
    let (status, response) = request(
        &app,
        "PUT",
        path,
        json!({"api_key":"secret-test-bailian"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["configured"], true);
    assert!(!response.to_string().contains("secret-test-bailian"));
    let (_, response) = request(&app, "GET", "/api/v1/voice/asr/providers", json!({}), None).await;
    assert!(!response.to_string().contains("secret-test-bailian"));
    assert!(
        response["result"]["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .all(|v| v.get("api_key").is_none()
                && v.get("access_token").is_none()
                && v.get("app_id").is_none())
    );
    let (status, response) = request(
        &app,
        "PUT",
        path,
        json!({"api_key":"","model":"paraformer-realtime-v2"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["configured"], true);
    let file = home.root().join("config/voice-asr-providers.json");
    assert!(
        std::fs::read_to_string(&file)
            .expect("private file")
            .contains("secret-test-bailian")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let (_, response) = request(&app, "PUT", path, json!({"clear_credentials":true}), None).await;
    assert_eq!(response["result"]["configured"], false);
    assert!(
        !std::fs::read_to_string(file)
            .expect("private file")
            .contains("secret-test-bailian")
    );
}

#[tokio::test]
async fn non_admin_cannot_read_mutate_or_probe_provider_configuration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("reader", vec!["g_test".into()], false, Some("reader-token"))
        .expect("reader token");
    let app = auth_support::authenticated_app(home);
    for (method, path) in [
        ("GET", "/api/v1/voice/asr/providers"),
        ("PUT", "/api/v1/voice/asr/providers/bailian"),
        ("POST", "/api/v1/voice/asr/providers/volcengine/probe"),
    ] {
        let (status, response) = request(
            &app,
            method,
            path,
            json!({"api_key":"should-not-save"}),
            Some("reader-token"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(response["error"]["code"], "admin_required");
    }
}

#[tokio::test]
async fn provider_settings_reject_arbitrary_endpoints_and_invalid_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let app = auth_support::authenticated_app(home.clone());
    for patch in [
        json!({"endpoint":"wss://untrusted.example"}),
        json!({"api_key":"secret\r\nheader"}),
        json!({"workspace_id":"x.evil.example"}),
    ] {
        let (status, response) = request(
            &app,
            "PUT",
            "/api/v1/voice/asr/providers/bailian",
            patch,
            None,
        )
        .await;
        assert!(!status.is_success());
        assert!(!response.to_string().contains("secret\r\nheader"));
    }
    assert!(
        !home.root().join("config/voice-asr-providers.json").exists(),
        "invalid configuration must not be persisted"
    );
}

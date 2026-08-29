mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::Value;
use std::process::Command;
use tower::ServiceExt;

#[tokio::test]
async fn scope_root_resolves_git_root_and_returns_complete_receipt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scope = temp.path().join("scope");
    let nested = scope.join("nested");
    std::fs::create_dir_all(&nested).expect("nested");
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&scope)
            .status()
            .expect("git init")
            .success()
    );
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let response = auth_support::authenticated_app(home)
        .oneshot(
            Request::get(format!(
                "/api/v1/fs/scope_root?path={}",
                nested.to_string_lossy()
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    let returned_path = std::path::PathBuf::from(payload["result"]["path"].as_str().expect("path"))
        .canonicalize()
        .expect("returned path");
    assert_eq!(returned_path, nested.canonicalize().expect("nested path"));
    let returned_root = std::path::PathBuf::from(
        payload["result"]["scope_root"]
            .as_str()
            .expect("scope root"),
    )
    .canonicalize()
    .expect("returned root");
    assert_eq!(returned_root, scope.canonicalize().expect("scope path"));
    assert!(
        payload["result"]["scope_key"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(payload["result"]["git_remote"].is_string());
}

#![cfg(unix)]
mod auth_support;
#[allow(dead_code)]
mod workspace_support;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn downloads_distinguish_files_internal_links_missing_targets_and_outside_targets() {
    let fixture = workspace_support::fixture();
    let dir = fixture.repo.join(".antigravitycli");
    std::fs::create_dir(&dir).expect("directory");
    std::fs::write(dir.join("normal.json"), b"{\"fixture\":true}").expect("file");
    let outside = fixture.repo.parent().expect("parent").join("external.json");
    std::fs::write(&outside, b"outside").expect("outside fixture");
    for (target, name) in [
        (dir.join("normal.json"), "inside.json"),
        (outside.with_file_name("missing.json"), "broken.json"),
        (outside.clone(), "outside.json"),
    ] {
        std::os::unix::fs::symlink(target, dir.join(name)).expect("fixture link");
    }
    let app = auth_support::authenticated_app(fixture.home.clone());
    for (name, expected) in [
        ("normal.json", StatusCode::OK),
        ("inside.json", StatusCode::OK),
        ("broken.json", StatusCode::NOT_FOUND),
        ("outside.json", StatusCode::FORBIDDEN),
    ] {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("scope_key", "scope_repo")
            .append_pair("scope_url", &fixture.repo.to_string_lossy())
            .append_pair("path", &format!(".antigravitycli/{name}"))
            .append_pair("download", "true")
            .finish();
        let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/v1/groups/{}/workspace/content?{query}",
                    fixture.group_id
                ))
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), 4096).await.expect("body");
        assert_eq!(status, expected, "{name}");
        if status == StatusCode::OK {
            assert_eq!(&body[..], b"{\"fixture\":true}");
        } else {
            let payload: serde_json::Value = serde_json::from_slice(&body).expect("error JSON");
            assert_eq!(
                payload["error"]["code"],
                if name == "broken.json" {
                    "NOT_FOUND"
                } else {
                    "outside_scope"
                }
            );
        }
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("scope_key", "scope_repo")
        .append_pair("scope_url", &fixture.repo.to_string_lossy())
        .append_pair("path", ".antigravitycli")
        .finish();
    let (status, payload) = workspace_support::json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{}/workspace/list?{query}",
            fixture.group_id
        ))
        .body(Body::empty())
        .expect("list request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = payload["result"]["items"].as_array().expect("entries");
    for (name, reason) in [
        ("broken.json", "missing"),
        ("outside.json", "outside_scope"),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["name"] == name)
            .expect("link entry");
        assert_eq!(entry["is_symlink"], true);
        assert_eq!(entry["unavailable"], reason);
        assert!(entry.get("size").is_none());
        assert!(entry.get("mime_type").is_none());
    }
}

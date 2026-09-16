mod auth_support;
#[allow(dead_code)]
mod workspace_support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use workspace_support::{fixture, json as call};

#[tokio::test]
async fn scoped_entry_lifecycle_and_streamed_upload_enforce_publication_boundaries() {
    let f = fixture();
    let app = auth_support::authenticated_app(f.home.clone());
    let base = format!("/api/v1/groups/{}/workspace", f.group_id);
    let scope = f
        .repo
        .canonicalize()
        .expect("root")
        .to_string_lossy()
        .into_owned();
    for (operation, expected) in [
        (
            json!({"operation":"create", "path":"new", "directory":true}),
            StatusCode::OK,
        ),
        (
            json!({"operation":"create", "path":"new", "directory":true}),
            StatusCode::CONFLICT,
        ),
        (
            json!({"operation":"create", "path":"../escape", "directory":false}),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let mut body = operation;
        body["scope_key"] = json!("scope_repo");
        body["scope_url"] = json!(scope);
        let (status, _) = call(
            &app,
            Request::post(format!("{base}/entries"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await;
        assert_eq!(status, expected);
    }
    for (name, expected_bytes, contents, expected_status) in [
        ("incomplete", 5, "ab", StatusCode::BAD_REQUEST),
        ("oversized", 1, "ab", StatusCode::BAD_REQUEST),
        ("ok.txt", 2, "ab", StatusCode::OK),
        ("ok.txt", 2, "cd", StatusCode::CONFLICT),
    ] {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("scope_key", "scope_repo")
            .append_pair("scope_url", &scope)
            .append_pair("path", &format!("new/{name}"))
            .append_pair("bytes", &expected_bytes.to_string())
            .finish();
        let (status, body) = call(
            &app,
            Request::post(format!("{base}/upload?{query}"))
                .body(Body::from(contents))
                .expect("request"),
        )
        .await;
        assert_eq!(status, expected_status, "{body}");
    }
    assert_eq!(
        std::fs::read(f.repo.join("new/ok.txt")).expect("uploaded"),
        b"ab"
    );
    assert_eq!(
        std::fs::read_dir(f.repo.join("new"))
            .expect("listing")
            .count(),
        1
    );
    let (status, renamed) = call(
        &app,
        Request::post(format!("{base}/entries"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"operation":"move", "path":"new/ok.txt", "destination":"new/ok.md",
            "scope_key":"scope_repo", "scope_url":scope})
                .to_string(),
            ))
            .expect("rename request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["result"]["mime_type"], "text/markdown");
    assert_eq!(renamed["result"]["destination"], "new/ok.md");
    for operation in [
        json!({"operation":"move", "path":"new", "destination":"renamed"}),
        json!({"operation":"delete", "path":"renamed"}),
    ] {
        let mut body = operation;
        body["scope_key"] = json!("scope_repo");
        body["scope_url"] = json!(scope);
        let (status, result) = call(
            &app,
            Request::post(format!("{base}/entries"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
    }
    assert!(!f.repo.join("renamed").exists());
}

#[tokio::test]
async fn entry_operations_keep_group_permissions_scope_and_exhibit_boundaries() {
    use cccc_core::access_tokens::AccessTokenStore;
    let f = fixture();
    let app = auth_support::authenticated_app(f.home.clone());
    let store = AccessTokenStore::new(f.home.clone()).expect("tokens");
    store
        .create(
            "scoped",
            vec![f.group_id.clone()],
            false,
            Some("allowed-fixture"),
        )
        .expect("allowed");
    store
        .create(
            "other",
            vec!["other-group".into()],
            false,
            Some("denied-fixture"),
        )
        .expect("denied");
    let path = format!("/api/v1/groups/{}/workspace/entries", f.group_id);
    let body = json!({"scope_key":"scope_repo", "scope_url":f.repo, "operation":"create", "path":"allowed.txt", "directory":false});
    for (token, url, scope, expected) in [
        (
            "denied-fixture",
            path.clone(),
            "scope_repo",
            StatusCode::FORBIDDEN,
        ),
        (
            "allowed-fixture",
            path.clone(),
            "old_scope",
            StatusCode::CONFLICT,
        ),
        (
            auth_support::TEST_ADMIN_TOKEN,
            format!("{path}?connect_frame=revoked"),
            "scope_repo",
            StatusCode::FORBIDDEN,
        ),
        (
            "allowed-fixture",
            path.clone(),
            "scope_repo",
            StatusCode::OK,
        ),
    ] {
        let mut body = body.clone();
        body["scope_key"] = json!(scope);
        let (status, result) = call(
            &app,
            Request::post(url)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await;
        assert_eq!(status, expected, "{result}");
    }
    let exhibit =
        auth_support::authenticated_app_with_mode(f.home.clone(), cccc_web::WebMode::Exhibit);
    let (status, _) = call(
        &exhibit,
        Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn uploaded_file_is_not_published_after_token_revocation() {
    use cccc_core::access_tokens::AccessTokenStore;
    use std::sync::Arc;
    use tokio::sync::Notify;
    let f = fixture();
    let app = auth_support::authenticated_app(f.home.clone());
    let tokens = AccessTokenStore::new(f.home.clone()).expect("tokens");
    let token = tokens
        .create(
            "upload",
            vec![f.group_id.clone()],
            false,
            Some("upload-fixture"),
        )
        .expect("token");
    let ready = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let ready_body = ready.clone();
    let release_body = release.clone();
    let body = Body::from_stream(futures_util::stream::once(async move {
        ready_body.notify_one();
        release_body.notified().await;
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"complete"))
    }));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("scope_key", "scope_repo")
        .append_pair("scope_url", &f.repo.to_string_lossy())
        .append_pair("path", "revoked.txt")
        .append_pair("bytes", "8")
        .finish();
    let request = Request::post(format!(
        "/api/v1/groups/{}/workspace/upload?{query}",
        f.group_id
    ))
    .header("authorization", "Bearer upload-fixture")
    .body(body)
    .expect("request");
    let response = tokio::spawn(async move { call(&app, request).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready.notified())
        .await
        .expect("body requested");
    tokens
        .delete(&cccc_core::access_tokens::token_id(&token.token))
        .expect("revoke");
    release.notify_one();
    let (status, _) = response.await.expect("response");
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!f.repo.join("revoked.txt").exists());
    assert!(!std::fs::read_dir(&f.repo).expect("listing").any(|e| {
        e.expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".cccc-upload-")
    }));
}

#[tokio::test]
async fn git_inspection_keeps_scope_and_group_boundaries_without_mutations() {
    let f = fixture();
    let app = auth_support::authenticated_app(f.home.clone());
    let scope = f.repo.to_string_lossy();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("scope_key", "scope_repo")
        .append_pair("scope_url", &scope)
        .finish();
    let base = format!("/api/v1/groups/{}/workspace", f.group_id);
    let (status, result) = call(
        &app,
        Request::get(format!("{base}/changes?{query}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["result"]["repository"], false);
    let tokens = cccc_core::access_tokens::AccessTokenStore::new(f.home.clone()).expect("tokens");
    tokens
        .create(
            "other",
            vec!["other-group".into()],
            false,
            Some("git-denied-fixture"),
        )
        .expect("token");
    for endpoint in ["changes", "diff"] {
        let url = format!("{base}/{endpoint}?{query}&path=src/lib.rs&side=worktree");
        let (status, _) = call(
            &app,
            Request::get(&url)
                .header("authorization", "Bearer git-denied-fixture")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &app,
            Request::get(url.replace("scope_repo", "old_scope"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
}

#[tokio::test]
async fn an_upload_cleans_up_after_another_request_moves_its_parent() {
    use std::sync::Arc;
    use tokio::sync::Notify;
    let f = fixture();
    std::fs::create_dir(f.repo.join("parent")).expect("parent");
    let app = auth_support::authenticated_app(f.home.clone());
    let ready = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let ready_body = ready.clone();
    let release_body = release.clone();
    let body = Body::from_stream(futures_util::stream::once(async move {
        ready_body.notify_one();
        release_body.notified().await;
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"complete"))
    }));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("scope_key", "scope_repo")
        .append_pair("scope_url", &f.repo.to_string_lossy())
        .append_pair("path", "parent/file.txt")
        .append_pair("bytes", "8")
        .finish();
    let request = Request::post(format!(
        "/api/v1/groups/{}/workspace/upload?{query}",
        f.group_id
    ))
    .body(body)
    .expect("upload");
    let upload_app = app.clone();
    let response = tokio::spawn(async move { call(&upload_app, request).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready.notified())
        .await
        .expect("body requested");
    let (status, result) = call(
        &app,
        Request::post(format!("/api/v1/groups/{}/workspace/entries", f.group_id))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"scope_key":"scope_repo", "scope_url":f.repo,
            "operation":"move", "path":"parent", "destination":"moved"})
                .to_string(),
            ))
            .expect("move"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    release.notify_one();
    let (status, _) = response.await.expect("upload response");
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        std::fs::read_dir(f.repo.join("moved"))
            .expect("moved entries")
            .count(),
        0
    );
}

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn public_session_reads_allowed_text_and_rejects_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join("docs")).expect("docs");
    std::fs::write(repo.join("docs/note.md"), "needle here\n").expect("note");
    std::fs::write(repo.join("docs/binary.md"), b"abc\0def").expect("binary");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("nomcp", "").expect("group");
    group_scope::attach(
        &groups,
        &group.group_id,
        Scope {
            scope_key: "scope_repo".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let admin = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let app = cccc_web::app(home);
    let created = app
        .clone()
        .oneshot(
            Request::post("/api/v1/nomcp/sessions")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"group_id":"{}","allowed_paths":["docs"]}}"#,
                    group.group_id
                )))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(created.status(), StatusCode::OK);
    let payload = json(created).await;
    let sid = payload["result"]["session"]["sid"].as_str().expect("sid");
    let secret = payload["result"]["secret"].as_str().expect("secret");

    let read = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/nomcp/s/{sid}/read?token={secret}&path=docs/note.md&format=json"
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(read.status(), StatusCode::OK);
    assert!(
        json(read).await["result"]["content"]
            .as_str()
            .is_some_and(|text| text.contains("needle"))
    );

    let escape = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/nomcp/s/{sid}/read?token={secret}&path=../outside&format=json"
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(escape.status(), StatusCode::BAD_REQUEST);

    let binary = app
        .oneshot(
            Request::get(format!(
                "/nomcp/s/{sid}/read?token={secret}&path=docs/binary.md&format=json"
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(binary.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

async fn json(response: axum::response::Response) -> Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

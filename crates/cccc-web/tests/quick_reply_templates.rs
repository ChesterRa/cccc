mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

const PATH: &str = "/api/v1/quick-reply-templates";

async fn request(
    app: &axum::Router,
    method: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(PATH)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map_or_else(String::new, |body| body.to_string()),
                ))
                .expect("request"),
        )
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
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

// The route passes only the saved fields on. The signed-in identity decides whose replies change,
// whatever the body names. Missing and null lists are refused, never read as deleting all.
#[tokio::test]
async fn a_save_changes_only_the_signed_in_persons_replies_and_passes_the_complete_list() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    let app = auth_support::authenticated_app(home.clone());
    let tokens = AccessTokenStore::new(home.clone()).expect("token store");
    let alice = tokens
        .create("alice", Vec::new(), false, None)
        .expect("alice token");
    let bob = tokens
        .create("bob", Vec::new(), false, None)
        .expect("bob token");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let address = cccc_daemon::DaemonPaths::new(home.clone()).address;
    for _ in 0..200 {
        if address.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(address.exists(), "isolated daemon started");

    let (status, saved) = request(
        &app,
        "PUT",
        &alice.token,
        Some(json!({
            "if_revision": 0,
            "replies": [{"name":"Go","text":"Go ahead."},{"builtin":"why","name":"Ask"}],
            "by": "peer1",
            "caller_id": "bob",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["result"]["revision"], 1);
    assert_eq!(
        saved["result"]["replies"][1],
        json!({"builtin":"why","name":"Ask"})
    );
    let (status, bobs) = request(&app, "GET", &bob.token, None).await;
    assert_eq!(status, StatusCode::OK, "{bobs}");
    assert_eq!(bobs["result"]["revision"], 0);
    assert_eq!(
        bobs["result"]["replies"],
        json!([{"builtin":"proceed"},{"builtin":"why"},{"builtin":"rethink"},{"builtin":"choices"}])
    );

    let (status, kept) = request(
        &app,
        "PUT",
        &alice.token,
        Some(json!({"if_revision":1,"replies":[]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["result"]["revision"], 2);
    assert_eq!(kept["result"]["replies"], json!([]));

    let (status, refused) = request(
        &app,
        "PUT",
        &alice.token,
        Some(json!({"if_revision":2,"replies":null})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["code"], "invalid_args");
    let (_, after) = request(&app, "GET", &alice.token, None).await;
    assert_eq!(after["result"]["revision"], 2);
    assert_eq!(after["result"]["replies"], json!([]));

    let client = cccc_client::DaemonClient::new(home);
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("shutdown response");
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon stopped")
        .expect("daemon task")
        .expect("daemon result");
}

cccc_daemon::session_owner_test_entry!();

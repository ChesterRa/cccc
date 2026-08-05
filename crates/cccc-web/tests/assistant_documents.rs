#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn document_get_reconciles_repository_edits_through_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).expect("repo");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice documents", "").expect("group");
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
    let saved = cccc_daemon::handle_request(
        &home,
        &DaemonRequest {
            v: 1,
            op: "assistant_voice_document_save".into(),
            args: json!({
                "group_id":group.group_id,
                "document_path":"docs/voice-secretary/meeting.md",
                "content":"old"
            })
            .as_object()
            .cloned()
            .expect("args"),
        },
    );
    assert!(saved.ok, "{saved:?}");
    std::fs::write(
        repo.join("docs/voice-secretary/meeting.md"),
        "# Meeting\n\nUpdated by Voice Secretary.\n",
    )
    .expect("external edit");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/documents?document_path=docs%2Fvoice-secretary%2Fmeeting.md",
                group.group_id
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["documents"][0]["content"],
        "# Meeting\n\nUpdated by Voice Secretary.\n"
    );
    assert_eq!(payload["result"]["documents"][0]["revision_count"], 2);

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

async fn wait_for_address(home: &HomeLayout) {
    let path = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("daemon address was not created");
}

#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, Scope};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn an_empty_closing_flush_with_blank_ids_is_recorded() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice document", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "scope".into(),
                url: workspace.to_string_lossy().into_owned(),
                label: "workspace".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "scope".into();
            Ok(())
        })
        .expect("scope");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    // What the composer sends when a document recording recognized nothing.
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/transcript_segments",
                group.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "session_id":"","segment_id":"","text":"","flush":true,"is_final":true,
                    "document_path":"docs/voice-secretary/empty.md"
                })
                .to_string(),
            ))
            .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert!(
        body["result"]["session_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(body["result"]["input_event"].is_null());
    assert!(body["result"]["secretary_task_id"].is_null());

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

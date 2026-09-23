mod auth_support;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn delete_route_reaches_daemon_and_removes_the_document_file() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("create fixture home");
    home.initialize().expect("initialize fixture home");
    let group = GroupStore::new(home.clone())
        .expect("open group store")
        .create("delete", "")
        .expect("create fixture group");
    let saved = cccc_daemon::handle_request(
        &home,
        &DaemonRequest {
            v: 1,
            op: "assistant_voice_document_save".into(),
            args:
                json!({"group_id":group.group_id,"document_path":"notes.md","content":"original"})
                    .as_object()
                    .expect("request object")
                    .clone(),
        },
    );
    assert!(saved.ok);
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    let ping = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Default::default(),
    };
    for _ in 0..100 {
        if client.call(&ping).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let app = auth_support::authenticated_app(home);
    let library_url = format!(
        "/api/v1/groups/{}/assistants/voice_secretary/documents/library",
        group.group_id
    );
    let created = app
        .clone()
        .oneshot(
            Request::post(&library_url)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"action":"create_folder","name":"Meeting notes"}).to_string(),
                ))
                .expect("build HTTP request"),
        )
        .await
        .expect("execute HTTP request");
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value = serde_json::from_slice(
        &created
            .into_body()
            .collect()
            .await
            .expect("read response body")
            .to_bytes(),
    )
    .expect("decode response JSON");
    let read = app
        .clone()
        .oneshot(
            Request::get(&library_url)
                .body(Body::empty())
                .expect("build HTTP request"),
        )
        .await
        .expect("execute HTTP request");
    assert_eq!(read.status(), StatusCode::OK);
    let read: Value = serde_json::from_slice(
        &read
            .into_body()
            .collect()
            .await
            .expect("read response body")
            .to_bytes(),
    )
    .expect("decode response JSON");
    assert_eq!(read["result"]["folders"], created["result"]["folders"]);
    assert_eq!(read["result"]["folders"][0]["name"], "Meeting notes");
    let response = app
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/assistants/voice_secretary/documents/delete",
                group.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"document_path":"notes.md"}).to_string()))
            .expect("build HTTP request"),
        )
        .await
        .expect("execute HTTP request");
    let status = response.status();
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("read response body")
            .to_bytes(),
    )
    .expect("decode response JSON");
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("send daemon request");
    daemon
        .await
        .expect("join daemon task")
        .expect("daemon exits successfully");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["document"]["status"], "deleted");
    assert!(body["result"]["document"]["trash_path"].is_null());
    assert!(
        !std::path::Path::new(
            saved.result["document"]["absolute_path"]
                .as_str()
                .expect("serialized document path")
        )
        .exists()
    );
}

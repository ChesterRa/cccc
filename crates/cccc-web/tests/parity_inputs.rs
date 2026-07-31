#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn terminal_clear_accepts_actor_id_from_query_without_json_body() {
    let (_temp, home, group_id, daemon) = running_home("terminal clear").await;
    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{group_id}/terminal/clear?actor_id=missing"
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    shutdown(home, daemon).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body).expect("json error response");
    assert_eq!(payload["error"]["code"], "runtime_error");
}

#[tokio::test]
async fn invalid_refs_json_is_rejected_before_upload_commit() {
    let (_temp, home, group_id, daemon) = running_home("invalid refs").await;
    let boundary = "cccc-invalid-refs";
    let multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nhello\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"refs_json\"\r\n\r\nnot-json\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"x.txt\"\r\n",
            "Content-Type: text/plain\r\n\r\npayload\r\n",
            "--{boundary}--\r\n"
        ),
        boundary = boundary,
    );
    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{group_id}/send_upload"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let payload = response_json(response).await;
    let blob_root = home
        .groups_dir()
        .join(&group_id)
        .join("state")
        .join("blobs");
    let blobs_empty = !blob_root.exists()
        || std::fs::read_dir(blob_root)
            .expect("blob directory")
            .next()
            .is_none();
    shutdown(home, daemon).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["error"]["code"], "invalid_refs");
    assert!(blobs_empty);
}

#[tokio::test]
async fn web_ack_is_fixed_to_user_identity() {
    let (_temp, home, group_id, daemon) = running_home("ack identity").await;
    let store = GroupStore::new(home.clone()).expect("store");
    let mut message = Event::new("chat.message", &group_id);
    message.id = "message-1".into();
    message.by = "user".into();
    message.data.insert("text".into(), json!("hello"));
    ledger::append(&store.ledger_path(&group_id).expect("ledger"), &message).expect("append");

    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{group_id}/events/message-1/ack"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"by":"peer-reviewer"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let payload = response_json(response).await;
    shutdown(home, daemon).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["result"]["event"]["by"], "user");
    assert_eq!(payload["result"]["event"]["data"]["actor_id"], "user");
}

#[tokio::test]
async fn actor_command_uses_shell_quoting_like_python() {
    let (_temp, home, group_id, daemon) = running_home("quoted command").await;
    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{group_id}/actors"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "actor_id":"quoted",
                        "runtime":"claude",
                        "runner":"pty",
                        "role":"peer",
                        "command":"claude --model \"model with spaces\""
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let payload = response_json(response).await;
    shutdown(home, daemon).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["result"]["actor"]["command"],
        json!(["claude", "--model", "model with spaces"])
    );
}

async fn running_home(
    title: &str,
) -> (
    tempfile::TempDir,
    HomeLayout,
    String,
    tokio::task::JoinHandle<()>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create(title, "")
        .expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move {
        cccc_daemon::run(daemon_home).await.expect("daemon");
    });
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            return (temp, home, group.group_id, daemon);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

async fn shutdown(_home: HomeLayout, daemon: tokio::task::JoinHandle<()>) {
    daemon.abort();
    let _ = daemon.await;
}

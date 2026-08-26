#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn terminal_clear_accepts_actor_id_from_query_without_json_body() {
    let (_temp, home, group_id, daemon) = running_home("terminal clear").await;
    let response = auth_support::authenticated_app(home.clone())
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
    assert_eq!(status, StatusCode::NOT_FOUND);
    let payload: Value = serde_json::from_slice(&body).expect("json error response");
    assert_eq!(payload["error"]["code"], "actor_not_found");
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
    let response = auth_support::authenticated_app(home.clone())
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
async fn group_update_http_surface_returns_the_standard_receipt() {
    let (_temp, home, group_id, daemon) = running_home("group update").await;
    let app = auth_support::authenticated_app(home.clone());
    let response = app
        .clone()
        .oneshot(
            Request::put(format!("/api/v1/groups/{group_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"updated title","topic":"updated topic","by":"user"})
                        .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let payload = response_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["result"]["group_id"], group_id);
    assert_eq!(payload["result"]["group"]["title"], "updated title");
    assert_eq!(
        payload["result"]["event"]["data"]["patch"],
        json!({"title":"updated title","topic":"updated topic"})
    );

    let no_change = app
        .oneshot(
            Request::put(format!("/api/v1/groups/{group_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":null,"by":"user"}"#))
                .expect("request"),
        )
        .await
        .expect("no-change response");
    let no_change_status = no_change.status();
    let no_change = response_json(no_change).await;
    shutdown(home, daemon).await;
    assert_eq!(no_change_status, StatusCode::OK);
    assert_eq!(no_change["result"]["message"], "no changes");
}

#[tokio::test]
async fn capability_install_http_surface_uses_the_canonical_daemon_operation() {
    let (_temp, home, group_id, daemon) = running_home("capability install").await;
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{group_id}/capabilities/install"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let payload = response_json(response).await;
    shutdown(home, daemon).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["error"]["code"], "missing_install_target");
}

#[tokio::test]
async fn message_control_http_routes_use_existing_event_operations() {
    let (_temp, home, group_id, daemon) = running_home("message controls").await;
    let app = auth_support::authenticated_app(home.clone());
    let deliver = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{group_id}/messages/missing-event/deliver"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"actor_ids":["peer-1"]}"#))
            .expect("request"),
        )
        .await
        .expect("response");
    let deliver_status = deliver.status();
    let deliver = response_json(deliver).await;
    let cancel = app
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{group_id}/messages/missing-event/reply-request/cancel"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("request"),
        )
        .await
        .expect("response");
    let cancel_status = cancel.status();
    let cancel = response_json(cancel).await;
    shutdown(home, daemon).await;

    assert_eq!(deliver_status, StatusCode::NOT_FOUND);
    assert_eq!(deliver["error"]["code"], "event_not_found");
    assert_eq!(cancel_status, StatusCode::NOT_FOUND);
    assert_eq!(cancel["error"]["code"], "event_not_found");
}

#[tokio::test]
async fn actor_command_uses_shell_quoting_like_python() {
    let (_temp, home, group_id, daemon) = running_home("quoted command").await;
    let response = auth_support::authenticated_app(home.clone())
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

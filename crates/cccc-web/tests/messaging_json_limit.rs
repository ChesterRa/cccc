#![cfg(unix)]

mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn message_json_larger_than_axum_default_limit_reaches_the_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("large json message", "")
        .expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let text = "x".repeat(3 * 1024 * 1024);
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send", group.group_id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"text":text,"by":"user","to":["user"],"message_mode":"send"})
                        .to_string(),
                ))
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
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let event = &payload["result"]["event"];
    let attachment = &event["data"]["attachments"][0];
    assert!(
        event["data"]["text"]
            .as_str()
            .is_some_and(|value| value.starts_with("[file] cccc-message-"))
    );
    assert_eq!(attachment["bytes"], text.len());
    assert_eq!(attachment["mime_type"], "text/plain;charset=utf-8");
    let path = attachment["path"].as_str().expect("attachment path");
    assert_eq!(
        std::fs::read(home.groups_dir().join(&group.group_id).join(path)).expect("blob"),
        text.as_bytes()
    );

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[tokio::test]
async fn oversized_reply_drops_the_client_quote_before_daemon_ipc() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("large json reply", "")
        .expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let target = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({
                "group_id":group.group_id,"text":"original target","by":"user","to":["user"],
                "message_mode":"send"
            })
            .as_object()
            .cloned()
            .expect("args"),
        })
        .await
        .expect("target request");
    let target_id = target.result["event"]["id"]
        .as_str()
        .expect("target id")
        .to_owned();
    let text = "回".repeat(1024 * 1024);
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/reply", group.group_id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "text":text,"quote_text":"q".repeat(3 * 1024 * 1024),"by":"user",
                        "to":["user"],"reply_to":target_id,"message_mode":"send"
                    })
                    .to_string(),
                ))
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
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let event = &payload["result"]["event"];
    assert_eq!(event["data"]["quote_text"], "original target");
    assert_eq!(event["data"]["attachments"][0]["bytes"], text.len());

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

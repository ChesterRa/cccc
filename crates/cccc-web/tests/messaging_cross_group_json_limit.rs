#![cfg(unix)]

mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::{Map, json};
use tower::ServiceExt;

#[tokio::test]
async fn multi_megabyte_local_cross_group_text_reaches_both_ledgers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let source = groups.create("source", "").expect("source");
    let destination = groups.create("destination", "").expect("destination");
    groups
        .mutate(&destination.group_id, |group| {
            group.actors.push(cccc_contracts::Actor::new("lead"));
            Ok(())
        })
        .expect("destination foreman");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let text = "x".repeat(3 * 1024 * 1024);

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/send_cross_group",
                source.group_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "text":text,"by":"user","dst_group_id":destination.group_id,
                    "to":["@foreman"],"message_mode":"send"
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

    for group_id in [&source.group_id, &destination.group_id] {
        let events =
            ledger::tail(&groups.ledger_path(group_id).expect("ledger"), 10).expect("tail");
        let message = events
            .iter()
            .find(|event| event.kind == "chat.message")
            .expect("chat message");
        assert_eq!(
            message.data["text"].as_str().map(str::len),
            Some(text.len())
        );
        assert!(
            message
                .data
                .get("attachments")
                .and_then(|value| value.as_array())
                .is_none_or(Vec::is_empty)
        );
    }

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("daemon address was not created");
}

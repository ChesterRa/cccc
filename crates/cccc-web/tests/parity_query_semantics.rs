#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn query_flags_change_final_actor_ledger_and_context_responses() {
    let (_temp, home, group_id, daemon) = running_home().await;
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(&group_id, |group| {
            let mut internal = Actor::new("internal-helper");
            internal.internal_kind = Some("helper".into());
            group.actors.push(internal);
            Ok(())
        })
        .expect("add internal actor");
    let mut message = Event::new("chat.message", &group_id);
    message.id = "query-message".into();
    message.by = "user".into();
    message.data.insert("to".into(), json!(["internal-helper"]));
    message.data.insert("text".into(), json!("hello"));
    ledger::append(&store.ledger_path(&group_id).expect("ledger"), &message).expect("append");

    let plain_actors = get(&home, format!("/api/v1/groups/{group_id}/actors")).await;
    let detailed_actors = get(
        &home,
        format!("/api/v1/groups/{group_id}/actors?include_internal=true&include_unread=true"),
    )
    .await;
    let plain_tail = get(
        &home,
        format!("/api/v1/groups/{group_id}/ledger/tail?kind=chat"),
    )
    .await;
    let status_tail = get(
        &home,
        format!(
            "/api/v1/groups/{group_id}/ledger/tail?kind=chat&with_read_status=true&with_ack_status=true&with_obligation_status=true"
        ),
    )
    .await;
    let summary = get(
        &home,
        format!("/api/v1/groups/{group_id}/context?detail=summary&fresh=true"),
    )
    .await;
    let full = get(
        &home,
        format!("/api/v1/groups/{group_id}/context?detail=full&fresh=true"),
    )
    .await;
    let invalid = get(
        &home,
        format!("/api/v1/groups/{group_id}/context?fresh=perhaps"),
    )
    .await;
    daemon.0.abort();

    assert!(actor(&plain_actors, "internal-helper").is_none());
    assert_eq!(
        actor(&detailed_actors, "internal-helper").expect("internal actor")["unread_count"],
        1
    );
    assert!(
        plain_tail["result"]["events"][0]
            .get("_read_status")
            .is_none()
    );
    assert!(status_tail["result"]["events"][0]["_read_status"].is_object());
    assert!(status_tail["result"]["events"][0]["_obligation_status"].is_object());
    assert!(summary["result"].get("board").is_none());
    assert!(full["result"]["board"].is_object());
    assert_eq!(invalid["status"], 400);
    assert_eq!(invalid["body"]["error"]["code"], "invalid_boolean");
}

fn actor<'a>(payload: &'a Value, id: &str) -> Option<&'a Value> {
    payload["result"]["actors"]
        .as_array()?
        .iter()
        .find(|actor| actor["id"] == id)
}

async fn get(home: &HomeLayout, path: String) -> Value {
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        cccc_web::app(home.clone())
            .oneshot(Request::get(&path).body(Body::empty()).expect("request")),
    )
    .await
    .unwrap_or_else(|_| panic!("request timed out: {path}"))
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
    if status == StatusCode::OK {
        body
    } else {
        json!({"status":status.as_u16(),"body":body})
    }
}

async fn running_home() -> (tempfile::TempDir, HomeLayout, String, DaemonGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("query semantics", "")
        .expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move {
        cccc_daemon::run(daemon_home).await.expect("daemon");
    });
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            return (temp, home, group.group_id, DaemonGuard(daemon));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

struct DaemonGuard(tokio::task::JoinHandle<()>);

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

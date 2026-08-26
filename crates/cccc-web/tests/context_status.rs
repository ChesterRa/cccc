mod auth_support;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn status_routes_return_ledger_derived_payloads() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("status routes", "").expect("group");
    let mut event = Event::new("chat.message", &group.group_id);
    event.id = "message-1".into();
    event.by = "user".into();
    event.data.insert("text".into(), json!("hello"));
    event.data.insert("to".into(), json!([]));
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("append event");
    let mut mail = Event::new("chat.message", &group.group_id);
    mail.id = "mail-1".into();
    mail.by = "user".into();
    mail.data.insert("text".into(), json!("read later"));
    mail.data.insert("to".into(), json!([]));
    mail.data.insert("message_mode".into(), json!("mail"));
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &mail)
        .expect("append mail");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home.clone());

    let batch = request_json(
        &app,
        Request::post(format!("/api/v1/groups/{}/ledger/statuses", group.group_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"event_ids":["message-1", "mail-1"]}).to_string(),
            ))
            .expect("batch request"),
    )
    .await;
    assert!(batch["result"]["statuses"]["message-1"].is_object());
    assert_eq!(
        batch["result"]["statuses"]["message-1"]["read_status"],
        Value::Null
    );
    assert_eq!(
        batch["result"]["statuses"]["mail-1"]["read_status"],
        json!({})
    );

    let single = request_json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{}/events/message-1/read_status",
            group.group_id
        ))
        .body(Body::empty())
        .expect("single request"),
    )
    .await;
    assert_eq!(single["result"]["event_id"], "message-1");
    assert_eq!(single["result"]["read_status"], json!({}));

    let mail_single = request_json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{}/events/mail-1/read_status",
            group.group_id
        ))
        .body(Body::empty())
        .expect("mail single request"),
    )
    .await;
    assert_eq!(mail_single["result"]["event_id"], "mail-1");
    assert_eq!(mail_single["result"]["read_status"], json!({}));

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

async fn wait_for_daemon(home: &HomeLayout) {
    let address = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if address.is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

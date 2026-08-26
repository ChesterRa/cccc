#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn changed_help_notifies_only_running_actors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(&home, "group_create", json!({"title":"help notify"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    for actor_id in ["running", "stopped"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"runtime":"custom","runner":"headless","role":"peer","by":"user"}),
        );
    }
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"running","by":"user"}),
    );
    call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"stopped","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = auth_support::authenticated_app(home.clone());
    let response = app
        .oneshot(
            Request::put(format!("/api/v1/groups/{group_id}/prompts/help"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"content":"updated help","by":"user"}).to_string(),
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
    daemon.abort();
    let _ = daemon.await;
    let payload: Value = serde_json::from_slice(&bytes).expect("json");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["result"]["notified_actor_ids"], json!(["running"]));
    assert_eq!(payload["result"]["notification_failures"], json!([]));
}

fn call(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{:?}", response.error);
    Value::Object(response.result)
}

async fn wait_for_daemon(home: &HomeLayout) {
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

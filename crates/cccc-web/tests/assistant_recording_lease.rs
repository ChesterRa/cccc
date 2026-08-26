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
async fn recording_lease_route_uses_the_daemon_owned_global_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("voice lease", "").expect("group");
    groups
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "assistants".into(),
                json!({"assistant":{"assistant_id":"voice_secretary","enabled":true}}),
            );
            Ok(())
        })
        .expect("enable assistant");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let app = auth_support::authenticated_app(home.clone());
    let path = format!(
        "/api/v1/groups/{}/assistants/voice_secretary/recording_lease",
        group.group_id
    );
    let acquired = app
        .clone()
        .oneshot(json_request(
            &path,
            json!({"action":"acquire","owner_id":"tab-a","ttl_seconds":30}),
        ))
        .await
        .expect("acquire response");
    assert_eq!(acquired.status(), StatusCode::OK);
    let acquired = json_body(acquired).await;
    let lease_id = acquired["result"]["lease_id"]
        .as_str()
        .expect("private lease token");
    assert!(acquired["result"]["lease"].get("lease_id").is_none());
    assert!(
        home.root()
            .join("state/voice_secretary_recording_lease.json")
            .is_file()
    );

    let conflict = app
        .clone()
        .oneshot(json_request(
            &path,
            json!({"action":"acquire","owner_id":"tab-b"}),
        ))
        .await
        .expect("conflict response");
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let conflict = json_body(conflict).await;
    assert_eq!(conflict["error"]["code"], "assistant_voice_recording_busy");
    assert!(
        conflict["error"]["details"]["active_lease"]
            .get("lease_id")
            .is_none()
    );

    let released = app
        .oneshot(json_request(
            &path,
            json!({"action":"release","owner_id":"tab-a","lease_id":lease_id}),
        ))
        .await
        .expect("release response");
    assert_eq!(released.status(), StatusCode::OK);
    assert_eq!(json_body(released).await["result"]["released"], true);

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

fn json_request(path: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).expect("request JSON")))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response JSON")
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

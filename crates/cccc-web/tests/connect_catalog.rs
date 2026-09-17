#![cfg(unix)]
mod auth_support;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn composer_catalog_obeys_web_authority_and_references_remain_local() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("Source", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            doc.actors.push(Actor::new("lead"));
            Ok(())
        })
        .expect("actor");
    let app = auth_support::authenticated_app(home.clone());
    AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create(
            "restricted",
            vec![group.group_id.clone()],
            false,
            Some("catalog-test-restricted"),
        )
        .expect("restricted token");
    let path = format!("/api/v1/groups/{}/connect/catalog", group.group_id);
    let request = |url: &str| Request::get(url).body(Body::empty()).expect("request");
    let denied = app
        .clone()
        .oneshot(
            Request::get(&path)
                .header(header::AUTHORIZATION, "Bearer catalog-test-restricted")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let exhibit =
        auth_support::authenticated_app_with_mode(home.clone(), cccc_web::WebMode::Exhibit);
    assert_eq!(
        exhibit
            .oneshot(request(&path))
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    // The ordinary Group resource wrapper must reject a retired frame before IPC.
    assert_eq!(
        app.clone()
            .oneshot(request(&format!("{path}?connect_frame=retired")))
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let response = app.clone().oneshot(request(&path)).await.expect("catalog");
    assert_eq!(response.status(), StatusCode::OK);
    let data: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(data["result"]["instances"], json!([]));
    assert_eq!(data["result"]["external_groups"], json!([]));
    let missing = app
        .clone()
        .oneshot(request(&format!(
            "{path}?instance_id=i_missing&target_group_id=g_remote&limit=64"
        )))
        .await
        .expect("response");
    let data: Value = serde_json::from_slice(
        &missing
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(data["error"]["code"], "connect_peer_unavailable");

    let reference = json!({"kind":"connect_group_ref", "instance_id":"i_mac", "group_id":"g_remote", "group_title":"Team", "token":"#Team · Mac"});
    let sent = app.oneshot(Request::post(format!("/api/v1/groups/{}/send", group.group_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"text":"Please ask #Team · Mac for help", "by":"user", "to":["lead"], "message_mode":"mail", "refs":[reference]}).to_string())).expect("send request"))
        .await.expect("send");
    let status = sent.status();
    let data = sent.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&data));
    let events =
        ledger::tail(&store.ledger_path(&group.group_id).expect("path"), 10).expect("ledger");
    let message = events
        .iter()
        .find(|event| event.kind == "chat.message")
        .expect("local message");
    assert_eq!(message.data["refs"][0], reference);
    assert_eq!(message.data["to"], json!(["lead"]));
    assert!(message.data.get("dst_instance_id").is_none());

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

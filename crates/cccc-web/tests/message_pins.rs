mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, access_tokens::AccessTokenStore, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map_or_else(String::new, |body| body.to_string()),
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
    (
        status,
        serde_json::from_slice(&bytes).expect("JSON response"),
    )
}

#[tokio::test]
async fn message_pins_are_shared_by_authorized_web_users_and_denied_to_others() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("group store");
    let group = groups.create("pinned messages", "").expect("group");
    let other = groups.create("other group", "").expect("other group");
    let mut message = Event::new("chat.message", &group.group_id);
    message.by = "worker".into();
    message.data = json!({"text":"fixture","to":["user"],"message_mode":"send"})
        .as_object()
        .expect("message data")
        .clone();
    let ledger_path = groups.ledger_path(&group.group_id).expect("ledger");
    ledger::append(&ledger_path, &message).expect("message");
    let ledger_before = std::fs::read(&ledger_path).expect("ledger before");
    let app = auth_support::authenticated_app(home.clone());
    let tokens = AccessTokenStore::new(home.clone()).expect("token store");
    let permitted = tokens
        .create("reader", vec![group.group_id.clone()], false, None)
        .expect("group token");
    let excluded = tokens
        .create("other", vec![other.group_id.clone()], false, None)
        .expect("other group token");

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let address = cccc_daemon::DaemonPaths::new(home.clone()).address;
    for _ in 0..200 {
        if address.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(address.exists(), "isolated daemon started");
    let path = format!("/api/v1/groups/{}/pins", group.group_id);
    let ids_path = format!("{path}?detail=ids");
    let (status, result) = request(
        &app,
        "POST",
        &ids_path,
        Some(&permitted.token),
        Some(json!({"event_id":message.id,"pinned":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["result"]["pins"][0]["event_id"], message.id);
    assert!(result["result"]["pins"][0].get("event").is_none());

    let (status, result) = request(&app, "GET", &path, None, None).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        result["result"]["pins"][0]["event"]["data"]["text"],
        "fixture"
    );
    assert_eq!(
        std::fs::read(&ledger_path).expect("ledger after"),
        ledger_before
    );

    for method in ["GET", "POST"] {
        let body = (method == "POST").then(|| json!({"event_id":message.id,"pinned":false}));
        let (status, _) = request(&app, method, &path, Some(&excluded.token), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "group access: {method}");
    }
    let exhibit =
        auth_support::authenticated_app_with_mode(home.clone(), cccc_web::WebMode::Exhibit);
    for method in ["GET", "POST"] {
        let body = (method == "POST").then(|| json!({"event_id":message.id,"pinned":false}));
        let (status, result) = request(&exhibit, method, &path, None, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "exhibit {method}: {result}");
        assert_eq!(result["error"]["code"], "read_only");
    }

    // Exhibit retains its existing export route, but an exported copy must not
    // reveal or transfer the Group's pinned-message state.
    let exported = exhibit
        .oneshot(
            Request::get(format!("/api/v1/groups/{}/copy/export", group.group_id))
                .body(Body::empty())
                .expect("export request"),
        )
        .await
        .expect("export response");
    assert_eq!(exported.status(), StatusCode::OK);
    let package = exported
        .into_body()
        .collect()
        .await
        .expect("export body")
        .to_bytes();
    let copied = cccc_core::group_copy::import(&groups, &package, "", "").expect("import copy");
    assert!(
        !groups
            .state_dir(&copied.group_id)
            .expect("copy state")
            .join("message_pins/pins.json")
            .exists()
    );
    assert!(
        groups
            .state_dir(&group.group_id)
            .expect("original state")
            .join("message_pins/pins.json")
            .exists()
    );

    let client = cccc_client::DaemonClient::new(home);
    client
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("shutdown response");
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon stopped")
        .expect("daemon task")
        .expect("daemon result");
}

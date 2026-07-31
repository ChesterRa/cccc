use axum::body::Body;
use axum::http::{Request, header};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn revoked_legacy_chat_stays_revoked_after_refresh() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("legacy IM", "").expect("group");
    std::fs::write(
        store
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("im_authorized_chats.json"),
        r#"{"chat-1":{"chat_id":"chat-1","thread_id":0,"platform":"wecom"}}"#,
    )
    .expect("legacy auth");
    std::fs::write(
        store
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("im_subscribers.json"),
        r#"{"chat-1":{"thread_id":0,"platform":"wecom","subscribed":true}}"#,
    )
    .expect("legacy subscriber");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let app = cccc_web::app(home);

    let imported = request(
        app.clone(),
        Request::get(format!("/api/im/authorized?group_id={}", group.group_id)),
        &token.token,
    )
    .await;
    assert_eq!(imported["result"]["authorized"][0]["chat_id"], "chat-1");

    let revoked = request(
        app.clone(),
        Request::post(format!(
            "/api/im/revoke?group_id={}&chat_id=chat-1&thread_id=0",
            group.group_id
        )),
        &token.token,
    )
    .await;
    assert_eq!(revoked["result"]["revoked"], true);
    assert_eq!(revoked["result"]["unsubscribed"], true);

    let refreshed = request(
        app,
        Request::get(format!("/api/im/authorized?group_id={}", group.group_id)),
        &token.token,
    )
    .await;
    assert_eq!(refreshed["result"]["authorized"], serde_json::json!([]));
    let state = cccc_core::integration_state::group_get(&store, &group.group_id, "im_bridge")
        .expect("canonical IM state");
    assert_eq!(state["authorized"], serde_json::json!([]));
    assert_eq!(state["subscribers"], serde_json::json!([]));
    assert!(
        store
            .state_dir(&group.group_id)
            .expect("state dir")
            .join("im_authorized_chats.json")
            .is_file(),
        "the legacy source remains present so the canonical empty arrays must prevent revival"
    );
}

async fn request(app: axum::Router, request: axum::http::request::Builder, token: &str) -> Value {
    let response = app
        .oneshot(
            request
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        response.status().is_success(),
        "status: {}",
        response.status()
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

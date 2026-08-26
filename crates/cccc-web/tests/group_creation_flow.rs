#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn recent_list_and_repeated_scope_create_complete_as_one_user_flow() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let admin = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let app = auth_support::authenticated_app(home.clone());

    let recent = get_json(&app, "/api/v1/fs/recent", &admin.token).await;
    assert!(
        recent["result"]["suggestions"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let parent = temp.path().canonicalize().expect("parent");
    let list_url = format!(
        "/api/v1/fs/list?path={}",
        utf8_percent_encode(&parent.to_string_lossy(), NON_ALPHANUMERIC)
    );
    let listed = get_json(&app, &list_url, &admin.token).await;
    assert_eq!(listed["result"]["path"], parent.to_string_lossy().as_ref());
    assert!(listed["result"]["items"].is_array());

    let target = temp.path().join("new-project");
    let created = app
        .clone()
        .oneshot(
            Request::post("/api/v1/groups")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Flow","path":target,"by":"user"}).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("create response");
    assert_eq!(created.status(), StatusCode::OK);
    let created = body_json(created).await;
    let group_id = created["result"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    assert_eq!(created["result"]["group"]["group_id"], group_id);
    let group = GroupStore::new(home.clone())
        .expect("store")
        .load(&group_id)
        .expect("group");
    assert_eq!(group.scopes.len(), 1);
    assert_eq!(
        group.scopes[0].url,
        target.canonicalize().expect("target").to_string_lossy()
    );
    let second = app
        .clone()
        .oneshot(
            Request::post("/api/v1/groups")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"title":"Second","path":target,"by":"user"}).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("second response");
    assert_eq!(second.status(), StatusCode::OK);
    let second = body_json(second).await;
    let second_id = second["result"]["group_id"]
        .as_str()
        .expect("second group id")
        .to_owned();
    assert_ne!(second_id, group_id);
    let second_group = GroupStore::new(home.clone())
        .expect("store")
        .load(&second_id)
        .expect("second group");
    assert_eq!(group.scopes[0], second_group.scopes[0]);
    assert_eq!(
        GroupStore::new(home.clone())
            .expect("store")
            .list()
            .expect("groups")
            .len(),
        2
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

async fn get_json(app: &axum::Router, path: &str, token: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::get(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
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

#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout, Registry, active, group_scope, scope};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

async fn post(
    app: &axum::Router,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&bytes).expect("JSON"))
}

#[tokio::test]
async fn http_nonactivating_attach_and_actor_save_preserve_group_and_project_selection() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let target = store.create("target", "").expect("target");
    let other = store.create("default owner", "").expect("other");
    let scopes = ["first", "second"].map(|name| {
        let path = temp.path().join(name);
        std::fs::create_dir(&path).expect("directory");
        scope::detect(&path).expect("scope")
    });
    group_scope::attach(&store, &target.group_id, scopes[0].clone()).expect("target scope");
    group_scope::attach(&store, &other.group_id, scopes[1].clone()).expect("other scope");
    active::set(&home, &other.group_id).expect("global selection");
    store
        .mutate(&target.group_id, |group| {
            group.actors = ["managed", "peer"]
                .map(|id| {
                    let mut actor = Actor::new(id);
                    actor.enabled = false;
                    actor
                })
                .into();
            Ok(())
        })
        .expect("disabled actors");
    let app = auth_support::authenticated_app(home.clone());
    let registry = Registry::load(&home).expect("registry before");
    let before = store.load(&target.group_id).expect("group before");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await.expect("daemon") });
    let address = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if address.is_file() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(address.is_file(), "isolated daemon started");
    let attach_path = format!("/api/v1/groups/{}/attach", target.group_id);
    for _ in 0..2 {
        let (status, receipt) = post(
            &app,
            &attach_path,
            json!({
                "group_id":other.group_id,"path":scopes[1].url,"activate":false,"by":"managed"
            }),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{receipt}");
        assert_eq!(
            receipt["result"]["group_id"], target.group_id,
            "URL owns Group identity"
        );
        assert_eq!(receipt["result"]["scope_key"], scopes[1].scope_key);
        assert_eq!(
            receipt["result"]["scope"],
            serde_json::to_value(&scopes[1]).expect("scope")
        );
        let attached = store.load(&target.group_id).expect("persisted attachment");
        assert_eq!(
            attached.scopes.len(),
            2,
            "repeat attach is not a duplicate scope"
        );
        assert_eq!(attached.active_scope_key, before.active_scope_key);
        assert_eq!(
            attached.actors, before.actors,
            "attach never saves an Actor"
        );
        let current_registry = Registry::load(&home).expect("registry");
        assert_eq!(current_registry.defaults, registry.defaults);
        assert_eq!(
            current_registry.groups[&target.group_id].default_scope_key,
            registry.groups[&target.group_id].default_scope_key
        );
        assert_eq!(
            active::get(&home).expect("global selection"),
            Some(other.group_id.clone())
        );
    }
    let (status, saved) = post(
        &app,
        &format!("/api/v1/groups/{}/actors/managed", target.group_id),
        json!({"by":"user","default_scope_key":scopes[1].scope_key}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["result"]["actor"]["default_scope_key"],
        scopes[1].scope_key
    );
    assert_eq!(saved["result"]["actor"]["enabled"], false);
    assert_eq!(
        store
            .load(&target.group_id)
            .expect("saved group")
            .active_scope_key,
        scopes[0].scope_key
    );
    assert_eq!(
        Registry::load(&home).expect("registry").defaults,
        registry.defaults
    );
    assert_eq!(
        active::get(&home).expect("global selection"),
        Some(other.group_id.clone())
    );

    let denied_before = store.load(&target.group_id).expect("before denied");
    let ledger_path = store.ledger_path(&target.group_id).expect("ledger path");
    let ledger = std::fs::read(&ledger_path).expect("ledger");
    for by in ["peer", "unknown"] {
        let (status, rejected) = post(
            &app,
            &attach_path,
            json!({"path":temp.path().join("missing"),"activate":false,"by":by}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{rejected}");
        assert_eq!(rejected["error"]["code"], "permission_denied");
    }
    let restricted = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create(
            "other group only",
            vec![other.group_id.clone()],
            false,
            None,
        )
        .expect("restricted token");
    let (status, _) = post(
        &app,
        &attach_path,
        json!({"path":scopes[1].url,"activate":false}),
        Some(&restricted.token),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        store.load(&target.group_id).expect("unchanged group"),
        denied_before
    );
    assert_eq!(
        std::fs::read(&ledger_path).expect("unchanged ledger"),
        ledger
    );

    let (status, activated) = post(
        &app,
        &attach_path,
        json!({"path":scopes[1].url,"by":"user"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{activated}");
    assert_eq!(
        store
            .load(&target.group_id)
            .expect("active group")
            .active_scope_key,
        scopes[1].scope_key
    );
    assert_eq!(
        Registry::load(&home).expect("claimed registry").defaults[&scopes[1].scope_key],
        target.group_id
    );
    assert_eq!(
        active::get(&home).expect("selected Group"),
        Some(target.group_id)
    );
    cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await
        .expect("shutdown");
    daemon.await.expect("daemon task");
}

#[tokio::test]
async fn exhibit_mode_rejects_nonactivating_attach_without_creating_state() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("read only", "").expect("group");
    let before = store.load(&group.group_id).expect("before");
    let app = auth_support::authenticated_app_with_mode(home, cccc_web::WebMode::Exhibit);
    let (status, response) = post(
        &app,
        &format!("/api/v1/groups/{}/attach", group.group_id),
        json!({"path":temp.path(),"activate":false}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(response["error"]["code"], "read_only");
    assert_eq!(store.load(&group.group_id).expect("unchanged"), before);
}

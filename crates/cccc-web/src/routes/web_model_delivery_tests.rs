use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tokio::sync::broadcast;
use tower::ServiceExt;

use super::web_model_delivery_test_support::{chrome_available, prompt_page};

#[tokio::test]
async fn connector_activity_binding_and_browser_delivery_share_one_turn() {
    if !chrome_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_sync(&home, "group_create", json!({"title":"web model e2e"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_sync(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","role":"peer","by":"user"}),
    );
    daemon_sync(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let (shutdown, _) = broadcast::channel(2);
    let (app, _, surfaces) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding::from_env(),
    );
    let (page_url, page_server) = prompt_page().await;
    surfaces
        .open(
            &super::web_model_browser::key(&group_id, "web1"),
            &temp.path().join("profile"),
            &page_url,
            800,
            600,
        )
        .await
        .expect("browser surface");

    let create = request_json(
        &app,
        Request::post("/api/v1/web-model/connectors")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","provider":"chatgpt"}).to_string(),
            ))
            .expect("create request"),
    )
    .await;
    let connector_id = create["result"]["connector"]["connector_id"]
        .as_str()
        .expect("connector id");
    let secret = create["result"]["secret"].as_str().expect("secret");
    let probe_status = app
        .clone()
        .oneshot(
            Request::get(format!("/mcp/web-model/{connector_id}?token={secret}"))
                .body(Body::empty())
                .expect("probe request"),
        )
        .await
        .expect("probe")
        .status();
    let bind = request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/bind-current")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"group_id":group_id,"actor_id":"web1","conversation_url":page_url})
                    .to_string(),
            ))
            .expect("bind request"),
    )
    .await;
    let client = DaemonClient::new(home.clone());
    let sent = client
        .call(&request(
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":"hello browser"}),
        ))
        .await
        .expect("send");
    let inspected = wait_for_background_delivery(&app, &group_id, "web1").await;
    let page = surfaces
        .sessions
        .lock()
        .await
        .get(&super::web_model_browser::key(&group_id, "web1"))
        .expect("session")
        .page
        .clone();
    let submitted: String = page
        .evaluate("globalThis.submitted || ''")
        .await
        .expect("submitted value")
        .into_value()
        .expect("submitted string");
    let connectors = request_json(
        &app,
        Request::get("/api/v1/web-model/connectors")
            .body(Body::empty())
            .expect("list request"),
    )
    .await;
    let _ = shutdown.send(());
    let _ = surfaces
        .close(&super::web_model_browser::key(&group_id, "web1"))
        .await;
    daemon.abort();
    let _ = daemon.await;
    page_server.abort();

    assert_eq!(probe_status, StatusCode::OK);
    assert!(sent.ok);
    assert_eq!(
        bind["result"]["browser_session"]["delivery_target"]["kind"],
        "existing_chat"
    );
    assert!(submitted.contains("hello browser"), "{submitted}");
    assert_eq!(
        inspected["result"]["browser_session"]["delivery_target"]["last_delivery_status"],
        "submitted"
    );
    assert_eq!(
        connectors["result"]["connectors"][0]["last_call_status"],
        "submitted"
    );
}

fn daemon_sync(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(home, &request(op, args));
    assert!(response.ok, "{:?}", response.error);
    Value::Object(response.result)
}

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    }
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.expect("response");
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

async fn wait_for_background_delivery(app: &axum::Router, group_id: &str, actor_id: &str) -> Value {
    for _ in 0..50 {
        let value = request_json(
            app,
            Request::get(format!(
                "/api/v1/web-model/browser-session?group_id={group_id}&actor_id={actor_id}"
            ))
            .body(Body::empty())
            .expect("browser state request"),
        )
        .await;
        if value["result"]["browser_session"]["delivery_target"]["last_delivery_status"]
            == "submitted"
        {
            return value;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("background browser delivery did not complete");
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

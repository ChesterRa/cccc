use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn web_owned_browser_surfaces_keep_product_routes_and_durable_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = daemon_call(&home, "group_create", json!({"title":"browser boundary"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    daemon_call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "runner":"headless",
            "role":"peer",
            "by":"user"
        }),
    );

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = cccc_web::app(home.clone());
    let presentation = request_json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group_id}/presentation/browser_surface/session?slot=slot-1"
        ))
        .body(Body::empty())
        .expect("presentation request"),
    )
    .await;
    assert_eq!(presentation["status"], StatusCode::OK.as_u16());
    assert_eq!(
        presentation["body"]["result"]["browser_surface"]["state"],
        "idle"
    );

    let provider = request_json(
        &app,
        Request::get("/api/v1/space/providers/notebooklm/auth")
            .body(Body::empty())
            .expect("provider request"),
    )
    .await;
    assert_eq!(provider["status"], StatusCode::OK.as_u16());
    assert_eq!(provider["body"]["result"]["provider"], "notebooklm");

    let bound = request_json(
        &app,
        Request::post("/api/v1/web-model/browser-session/bind-current")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "group_id":group_id,
                    "actor_id":"web1",
                    "conversation_url":"https://chatgpt.com/c/persisted-target"
                })
                .to_string(),
            ))
            .expect("bind request"),
    )
    .await;
    assert_eq!(bound["status"], StatusCode::OK.as_u16());
    assert_eq!(
        bound["body"]["result"]["browser_session"]["conversation_url"],
        "https://chatgpt.com/c/persisted-target"
    );

    drop(app);
    let restarted_app = cccc_web::app(home.clone());
    let restored = request_json(
        &restarted_app,
        Request::get(format!(
            "/api/v1/web-model/browser-session?group_id={group_id}&actor_id=web1"
        ))
        .body(Body::empty())
        .expect("restored target request"),
    )
    .await;
    assert_eq!(restored["status"], StatusCode::OK.as_u16());
    assert_eq!(
        restored["body"]["result"]["browser_session"]["conversation_url"],
        "https://chatgpt.com/c/persisted-target"
    );
    assert_eq!(
        restored["body"]["result"]["browser_surface"]["active"],
        false
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

fn daemon_call(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.to_owned(),
            args: args.as_object().cloned().expect("object args"),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("json body");
    json!({"status":status,"body":body})
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

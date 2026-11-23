use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use cccc_core::{HomeLayout, access_tokens::AccessTokenStore, local_sessions};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_management_routes_require_admin_and_project_removed_sources_without_erasing_lifecycle()
 {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let session:local_sessions::NativeSession=serde_json::from_value(json!({"source_id":"codex:route","runtime":"codex","native_session_id":"route","host":local_sessions::NativeHost::capture(std::process::id()).expect("host"),"cwd":temp.path(),"title":"route","revision":"original"})).expect("source");
    local_sessions::write(&home, session).expect("register");
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let admin = tokens.create("admin", vec![], true, None).expect("admin");
    let member = tokens
        .create("scope", vec!["g_allowed".into()], false, None)
        .expect("scoped token");
    for (mode, token, expected) in [
        (
            cccc_web::WebMode::Normal,
            Some(member.token.as_str()),
            StatusCode::FORBIDDEN,
        ),
        (
            cccc_web::WebMode::Exhibit,
            Some(admin.token.as_str()),
            StatusCode::FORBIDDEN,
        ),
        (cccc_web::WebMode::Normal, None, StatusCode::UNAUTHORIZED),
    ] {
        for route in ["control", "recheck"] {
            let mut request = Request::post(format!("/api/v1/local_sessions/{route}"))
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(token) = token {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let response=cccc_web::app_with_mode(home.clone(),mode).oneshot(request.body(Body::from(json!({"source_id":"codex:route","action":"remove","expected_revision":"0"}).to_string())).expect("request")).await.expect("response");
            assert_eq!(response.status(), expected);
        }
    }
    assert!(local_sessions::read(&home, "codex:route").is_ok());
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_err()
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let app = cccc_web::app(home.clone());
    let post = |body: Value| {
        Request::post("/api/v1/local_sessions/control")
            .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    };
    let response = app
        .clone()
        .oneshot(post(
            json!({"source_id":"codex:route","action":"remove","expected_revision":"0"}),
        ))
        .await
        .expect("remove");
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("JSON");
    assert_eq!(body["result"]["status"], "removed");
    assert_eq!(
        body["result"]["client_requirement"],
        "matching_build_and_reloaded_mcp"
    );
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/local_sessions")
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .expect("list"),
        )
        .await
        .expect("list response");
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("JSON");
    assert_eq!(body["result"]["capabilities"]["source_control"], true);
    assert_eq!(
        body["result"]["capabilities"]["source_control_requires_mcp_reload"],
        true
    );
    assert!(
        body["result"]["sources"]
            .as_array()
            .expect("sources")
            .is_empty()
    );
    assert_eq!(
        body["result"]["removed_sources"][0]["source_id"],
        "codex:route"
    );
    assert_eq!(
        body["result"]["source_states"]["codex:route"]["control"]["access_blocked"],
        true
    );
    let response = app
        .clone()
        .oneshot(post(
            json!({"source_id":"codex:route","action":"restore","expected_revision":"0"}),
        ))
        .await
        .expect("stale restore");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    for (id, status) in [
        ("codex:unknown", StatusCode::NOT_FOUND),
        ("../../project", StatusCode::BAD_REQUEST),
        ("remote:device:grant:source", StatusCode::BAD_REQUEST),
    ] {
        let response = app
            .clone()
            .oneshot(post(
                json!({"source_id":id,"action":"remove","expected_revision":"0"}),
            ))
            .await
            .expect("invalid source");
        assert_eq!(response.status(), status);
    }
    assert!(
        local_sessions::read_record(&home, "codex:route")
            .expect("retained metadata")
            .retired
            .is_none()
    );
    client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Default::default(),
        })
        .await
        .expect("stop test daemon");
    tokio::time::timeout(std::time::Duration::from_secs(5), daemon)
        .await
        .expect("stopped")
        .expect("task")
        .expect("result");
}

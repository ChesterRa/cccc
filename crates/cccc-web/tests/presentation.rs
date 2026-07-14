#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn presentation_http_routes_cover_url_upload_workspace_asset_and_clear() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join("docs")).expect("docs");
    std::fs::write(repo.join("docs/report.html"), "<h1>v1</h1>").expect("report");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("presentation", "").expect("group");
    group_scope::attach(
        &groups,
        &group.group_id,
        Scope {
            scope_key: "scope_repo".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let app = cccc_web::app(home.clone());

    let url = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/publish",
            group.group_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"slot":"slot-3","url":"https://example.com/dashboard","by":"user"}"#,
        ))
        .expect("request"),
    )
    .await;
    assert_eq!(url["result"]["card"]["card_type"], "web_preview");

    let workspace = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/publish_workspace",
            group.group_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"slot":"slot-1","path":"docs/report.html","by":"user"}"#,
        ))
        .expect("request"),
    )
    .await;
    assert_eq!(
        workspace["result"]["card"]["content"]["mode"],
        "workspace_link"
    );

    let asset = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/presentation/slots/slot-1/asset",
                group.group_id
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("asset");
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(asset.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        asset.into_body().collect().await.expect("body").to_bytes(),
        "<h1>v1</h1>"
    );

    let listing = request_json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{}/presentation/workspace/list",
            group.group_id
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert!(
        listing["result"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|item| item["name"] == "docs")
    );

    let boundary = "cccc-boundary";
    let multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"slot\"\r\n\r\nslot-2\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.md\"\r\nContent-Type: text/markdown\r\n\r\n# hello\r\n--{boundary}--\r\n"
    );
    let uploaded = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/publish_upload",
            group.group_id
        ))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(multipart))
        .expect("request"),
    )
    .await;
    assert_eq!(uploaded["result"]["card"]["title"], "notes.md");
    assert_eq!(uploaded["result"]["card"]["content"]["markdown"], "# hello");

    let cleared = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/clear",
            group.group_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"slot":"slot-2","by":"user"}"#))
        .expect("request"),
    )
    .await;
    assert_eq!(cleared["result"]["cleared_slots"], json!(["slot-2"]));

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

async fn request_json(app: &axum::Router, request: Request<Body>) -> Value {
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["ok"], true, "{value}");
    value
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

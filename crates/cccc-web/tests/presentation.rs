#![cfg(unix)]
mod auth_support;

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
    let app = auth_support::authenticated_app(home.clone());

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

    let pdf_url = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/publish",
            group.group_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"slot":"slot-3","url":"https://example.com/report.pdf?rev=2","by":"user"}"#,
        ))
        .expect("request"),
    )
    .await;
    assert_eq!(pdf_url["result"]["card"]["card_type"], "pdf");

    let image_url = request_json(
        &app,
        Request::post(format!(
            "/api/v1/groups/{}/presentation/publish",
            group.group_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"slot":"slot-3","url":"https://example.com/chart.PNG#latest","by":"user"}"#,
        ))
        .expect("request"),
    )
    .await;
    assert_eq!(image_url["result"]["card"]["card_type"], "image");

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
        "--{boundary}\r\nContent-Disposition: form-data; name=\"slot\"\r\n\r\nslot-2\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"by\"\r\n\r\n\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"notes.md\"\r\nContent-Type: text/markdown\r\n\r\n# hello\r\n--{boundary}--\r\n"
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
    assert_eq!(uploaded["result"]["card"]["published_by"], "user");
    assert_eq!(uploaded["result"]["card"]["source_ref"], "notes.md");
    assert_eq!(uploaded["result"]["event"]["by"], "user");

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

#[tokio::test]
async fn presentation_upload_routes_enforce_size_group_and_slot_boundaries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups
        .create("presentation upload boundaries", "")
        .expect("group");
    let app = auth_support::authenticated_app(home.clone());

    let accepted = app
        .clone()
        .oneshot(multipart_request(
            format!(
                "/api/v1/groups/{}/presentation/ref_snapshot",
                group.group_id
            ),
            "accepted-boundary",
            &[("slot", "slot-2")],
            "snapshot.jpg",
            "image/jpeg",
            &vec![b'x'; 3 * 1024 * 1024],
        ))
        .await
        .expect("accepted upload response");
    assert_eq!(accepted.status(), StatusCode::OK);

    let oversized = app
        .clone()
        .oneshot(multipart_request(
            format!(
                "/api/v1/groups/{}/presentation/ref_snapshot",
                group.group_id
            ),
            "oversized-boundary",
            &[("slot", "slot-2")],
            "snapshot.jpg",
            "image/jpeg",
            &vec![b'y'; 20 * 1024 * 1024 + 1],
        ))
        .await
        .expect("oversized upload response");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let oversized_body = oversized
        .into_body()
        .collect()
        .await
        .expect("oversized body")
        .to_bytes();
    let oversized_json: Value = serde_json::from_slice(&oversized_body).expect("oversized JSON");
    assert_eq!(oversized_json["error"]["code"], "file_too_large");

    let invalid_slot = app
        .clone()
        .oneshot(multipart_request(
            format!(
                "/api/v1/groups/{}/presentation/ref_snapshot",
                group.group_id
            ),
            "invalid-slot-boundary",
            &[("slot", "slot-9")],
            "invalid-slot.jpg",
            "image/jpeg",
            b"invalid-slot",
        ))
        .await
        .expect("invalid slot response");
    assert_eq!(invalid_slot.status(), StatusCode::BAD_REQUEST);

    for path in [
        "/api/v1/groups/g_missing/presentation/ref_snapshot",
        "/api/v1/groups/g_missing/presentation/publish_upload",
    ] {
        let missing = app
            .clone()
            .oneshot(multipart_request(
                path,
                "missing-group-boundary",
                &[("slot", "slot-1"), ("by", "user")],
                "missing.png",
                "image/png",
                b"missing-group",
            ))
            .await
            .expect("missing group response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(!home.groups_dir().join("g_missing").exists(), "{path}");
    }
}

fn multipart_request(
    uri: impl AsRef<str>,
    boundary: &str,
    fields: &[(&str, &str)],
    file_name: &str,
    mime: &str,
    data: &[u8],
) -> Request<Body> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::post(uri.as_ref())
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .expect("multipart request")
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

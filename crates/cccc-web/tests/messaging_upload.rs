#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, integration_state};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\ncccc-test-image";

#[tokio::test]
async fn plural_files_field_creates_a_structured_image_attachment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("image upload", "").expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let boundary = "cccc-message-boundary";
    let mut multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"by\"\r\n\r\nuser\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nimage message\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"user\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nrequest_reply\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"refs_json\"\r\n\r\n[]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"overview.png\"\r\n",
            "Content-Type: image/png\r\n\r\n"
        ),
        boundary = boundary,
    )
    .into_bytes();
    multipart.extend_from_slice(PNG);
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let response_body = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response_body)
    );
    let body = response_body;
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["ok"], true, "{payload}");

    let event = &payload["result"]["event"];
    assert_eq!(event["data"]["message_mode"], "request_reply");
    assert_eq!(event["data"]["refs"], json!([]));
    assert!(event["data"].get("files").is_none());
    let attachment = &event["data"]["attachments"][0];
    assert_eq!(attachment["title"], "overview.png");
    assert_eq!(attachment["mime_type"], "image/png");
    assert_eq!(attachment["bytes"], PNG.len());
    let path = attachment["path"].as_str().expect("attachment path");
    assert!(path.starts_with("state/blobs/"));
    let blob_name = path.rsplit('/').next().expect("blob name").to_owned();

    let blob = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/blobs/{blob_name}",
                group.group_id
            ))
            .body(Body::empty())
            .expect("blob request"),
        )
        .await
        .expect("blob response");
    assert_eq!(blob.status(), StatusCode::OK);
    assert_eq!(blob.headers()[header::CONTENT_TYPE], "image/png");
    assert!(!blob.headers().contains_key(header::CONTENT_DISPOSITION));
    assert_eq!(
        blob.into_body()
            .collect()
            .await
            .expect("blob body")
            .to_bytes(),
        PNG
    );

    let download = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/blobs/{blob_name}?filename=overview.png&download=true",
                group.group_id
            ))
            .body(Body::empty())
            .expect("download request"),
        )
        .await
        .expect("download response");
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(download.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        download.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"overview.png\"; filename*=UTF-8''overview%2Epng"
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

#[tokio::test]
async fn upload_larger_than_axum_default_limit_is_streamed_successfully() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("large upload", "").expect("group");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let boundary = "cccc-large-boundary";
    let mut multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nlarge\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"user\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nsend\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"large.bin\"\r\n",
            "Content-Type: application/octet-stream\r\n\r\n"
        ),
        boundary = boundary,
    )
    .into_bytes();
    multipart.extend(std::iter::repeat_n(b'x', 3 * 1024 * 1024));
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

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
async fn cross_group_upload_requires_destination_before_persisting_blob() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("remote upload", "").expect("group");
    let blobs_dir = groups
        .group_dir(&group.group_id)
        .expect("group directory")
        .join("state/blobs");
    let before = std::fs::read_dir(&blobs_dir)
        .expect("blob directory")
        .count();
    for (label, destination, destination_before_file) in [
        ("missing", None, false),
        ("empty-destination-first", Some(""), true),
        ("blank-destination-first", Some("   "), true),
        ("blank-file-first", Some("   "), false),
    ] {
        let boundary = format!("cccc-cross-group-{label}");
        let multipart =
            invalid_cross_group_multipart(&boundary, destination, destination_before_file);
        let response = auth_support::authenticated_app(home.clone())
            .oneshot(
                Request::post(format!(
                    "/api/v1/groups/{}/send_cross_group_upload",
                    group.group_id
                ))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{label}");
        assert_eq!(
            std::fs::read_dir(&blobs_dir)
                .expect("blob directory")
                .count(),
            before,
            "{label}"
        );
    }
}

#[tokio::test]
async fn cross_group_upload_preflights_mode_before_persisting_blob() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("remote upload preflight", "").expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({"trusts":[{
            "trust_id":"trust-upload","registration_id":"registration-upload",
            "group_id":group.group_id,"remote_group_id":"g_remote",
            "remote_peer_id":"peer-remote","status":"active",
            "remote_access_level":"messages"
        }]});
        Ok(())
    })
    .expect("bridge state");
    let blobs_dir = groups
        .group_dir(&group.group_id)
        .expect("group directory")
        .join("state/blobs");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let boundary = "cccc-invalid-cross-mode";
    let multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"dst_group_id\"\r\n\r\ng_remote\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nremote\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"peer1\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nlater\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"orphan.bin\"\r\n",
            "Content-Type: application/octet-stream\r\n\r\nmust not persist\r\n",
            "--{boundary}--\r\n"
        ),
        boundary = boundary,
    );
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!(
                "/api/v1/groups/{}/send_cross_group_upload",
                group.group_id
            ))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(multipart))
            .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !blobs_dir.exists()
            || std::fs::read_dir(&blobs_dir)
                .expect("blob directory")
                .next()
                .is_none()
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

#[tokio::test]
async fn reply_upload_rejects_nested_reply_mode_before_persisting_blob() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("invalid reply mode", "").expect("group");
    let blobs_dir = groups
        .group_dir(&group.group_id)
        .expect("group directory")
        .join("state/blobs");
    let before = std::fs::read_dir(&blobs_dir)
        .expect("blob directory")
        .count();
    let boundary = "cccc-invalid-reply-mode";
    let mut multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nreply\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"reply_to\"\r\n\r\nevent-1\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nrequest_reply\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"reply.bin\"\r\n",
            "Content-Type: application/octet-stream\r\n\r\n"
        ),
        boundary = boundary,
    )
    .into_bytes();
    multipart.extend_from_slice(b"reply payload");
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let response = auth_support::authenticated_app(home)
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/reply_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        std::fs::read_dir(&blobs_dir)
            .expect("blob directory")
            .count(),
        before
    );
}

#[tokio::test]
async fn send_upload_preflights_audience_before_persisting_blob() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("invalid upload audience", "").expect("group");
    let blobs_dir = groups
        .group_dir(&group.group_id)
        .expect("group directory")
        .join("state/blobs");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let boundary = "cccc-invalid-upload-audience";
    let multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nupload\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"user\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nmail\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"orphan.bin\"\r\n",
            "Content-Type: application/octet-stream\r\n\r\nmust not persist\r\n",
            "--{boundary}--\r\n"
        ),
        boundary = boundary,
    );
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        !blobs_dir.exists()
            || std::fs::read_dir(&blobs_dir)
                .expect("blob directory")
                .next()
                .is_none()
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

#[tokio::test]
async fn send_upload_replays_client_id_before_persisting_blob() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("upload replay", "").expect("group");
    let blobs_dir = groups
        .group_dir(&group.group_id)
        .expect("group directory")
        .join("state/blobs");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let accepted = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({
                "group_id":group.group_id,
                "by":"user",
                "text":"accepted",
                "to":["user"],
                "message_mode":"send",
                "client_id":"upload-replay"
            })
            .as_object()
            .cloned()
            .expect("args"),
        })
        .await
        .expect("accepted send");
    assert!(accepted.ok);
    let accepted_id = accepted.result["event"]["id"]
        .as_str()
        .expect("accepted id")
        .to_owned();

    let boundary = "cccc-upload-replay";
    let multipart = format!(
        concat!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nreplacement\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"user\"]\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nsend\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"client_id\"\r\n\r\nupload-replay\r\n",
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"orphan.bin\"\r\n",
            "Content-Type: application/octet-stream\r\n\r\nmust not persist\r\n",
            "--{boundary}--\r\n"
        ),
        boundary = boundary,
    );
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(payload["result"]["event"]["id"], accepted_id);
    assert_eq!(payload["result"]["duplicate"], true);
    assert!(
        !blobs_dir.exists()
            || std::fs::read_dir(&blobs_dir)
                .expect("blob directory")
                .next()
                .is_none()
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

fn invalid_cross_group_multipart(
    boundary: &str,
    destination: Option<&str>,
    destination_before_file: bool,
) -> Vec<u8> {
    let mut multipart =
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nremote\r\n")
            .into_bytes();
    if destination_before_file && let Some(destination) = destination {
        multipart.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"dst_group_id\"\r\n\r\n{destination}\r\n"
            )
            .as_bytes(),
        );
    }
    multipart.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"evidence.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    multipart.extend_from_slice(b"evidence\r\n");
    if !destination_before_file && let Some(destination) = destination {
        multipart.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"dst_group_id\"\r\n\r\n{destination}\r\n"
            )
            .as_bytes(),
        );
    }
    multipart.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    multipart
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

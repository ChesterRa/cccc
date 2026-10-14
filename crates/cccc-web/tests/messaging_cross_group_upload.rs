#![cfg(unix)]
mod auth_support;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\ncross-group-image";

fn multipart(boundary: &str, destination: &str) -> Vec<u8> {
    let mut body = format!(
        concat!(
            "--{b}\r\nContent-Disposition: form-data; name=\"by\"\r\n\r\nuser\r\n",
            "--{b}\r\nContent-Disposition: form-data; name=\"dst_group_id\"\r\n\r\n{d}\r\n",
            "--{b}\r\nContent-Disposition: form-data; name=\"to_json\"\r\n\r\n[\"user\"]\r\n",
            "--{b}\r\nContent-Disposition: form-data; name=\"message_mode\"\r\n\r\nsend\r\n",
            "--{b}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"chart.png\"\r\n",
            "Content-Type: image/png\r\n\r\n"
        ),
        b = boundary,
        d = destination,
    )
    .into_bytes();
    body.extend_from_slice(PNG);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

async fn post(
    home: &HomeLayout,
    group_id: &str,
    boundary: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    post_as(home, None, group_id, boundary, body).await
}

async fn post_as(
    home: &HomeLayout,
    token: Option<&str>,
    group_id: &str,
    boundary: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut request = Request::post(format!("/api/v1/groups/{group_id}/send_upload"));
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            request
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
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
    (status, serde_json::from_slice(&bytes).expect("json"))
}

fn blob_count(groups: &GroupStore, group_id: &str) -> usize {
    std::fs::read_dir(groups.state_dir(group_id).expect("state").join("blobs"))
        .map(|entries| entries.count())
        .unwrap_or(0)
}

#[tokio::test]
async fn oversized_upload_reports_its_limit_without_retaining_a_blob_or_message() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("upload limit", "").expect("group");
    let ledger = groups.ledger_path(&group.group_id).expect("ledger");
    let before = std::fs::read(&ledger).expect("ledger before");
    let boundary = "cccc-oversized-upload";
    let prefix = Bytes::from(format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"large.bin\"\r\n\r\n"
    ));
    let chunk = Bytes::from(vec![0_u8; 64 * 1024]);
    let suffix = Bytes::from(format!("x\r\n--{boundary}--\r\n"));
    let chunks = std::iter::once(prefix)
        .chain(std::iter::repeat_n(chunk, 1600))
        .chain(std::iter::once(suffix))
        .map(Ok::<_, std::convert::Infallible>);
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::post(format!("/api/v1/groups/{}/send_upload", group.group_id))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from_stream(futures_util::stream::iter(chunks)))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(payload["error"]["code"], "attachment_too_large");
    assert_eq!(blob_count(&groups, &group.group_id), 0);
    assert_eq!(std::fs::read(&ledger).expect("ledger after"), before);
}

#[tokio::test]
async fn upload_to_another_group_delivers_a_readable_image_there() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let source = groups.create("source", "").expect("source");
    let destination = groups.create("destination", "").expect("destination");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;

    let boundary = "cccc-cross-group-upload";
    let (status, payload) = post(
        &home,
        &source.group_id,
        boundary,
        multipart(boundary, &destination.group_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    let delivered = &payload["result"]["dst_event"];
    assert_eq!(delivered["group_id"], destination.group_id);
    let attachment = &delivered["data"]["attachments"][0];
    assert_eq!(attachment["kind"], "image");
    assert_eq!(attachment["title"], "chart.png");
    let name = attachment["path"]
        .as_str()
        .and_then(|path| path.rsplit('/').next())
        .expect("blob name")
        .to_owned();
    let blob = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/blobs/{name}",
                destination.group_id
            ))
            .body(Body::empty())
            .expect("blob request"),
        )
        .await
        .expect("blob response");
    assert_eq!(blob.status(), StatusCode::OK);
    assert_eq!(
        blob.into_body().collect().await.expect("blob").to_bytes(),
        PNG
    );

    // An unknown destination is refused before any upload is stored.
    let before = blob_count(&groups, &source.group_id);
    let boundary = "cccc-cross-group-missing";
    let (status, payload) = post(
        &home,
        &source.group_id,
        boundary,
        multipart(boundary, "g_missing"),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "{payload}");
    assert_eq!(blob_count(&groups, &source.group_id), before);

    // Scope validation must precede committing the staged upload as well.
    let boundary = "cccc-cross-group-scope";
    let mut body = multipart(boundary, &destination.group_id);
    let closing = format!("--{boundary}--\r\n");
    assert!(body.ends_with(closing.as_bytes()));
    body.truncate(body.len() - closing.len());
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"path\"\r\n\r\n{}\r\n{closing}",
            temp.path().display()
        )
        .as_bytes(),
    );
    let (status, payload) = post(&home, &source.group_id, boundary, body).await;
    assert_ne!(status, StatusCode::OK);
    assert_eq!(payload["error"]["code"], "scope_not_attached");
    assert_eq!(blob_count(&groups, &source.group_id), before);

    // A member of the source Group alone cannot upload into another Group.
    let member = cccc_core::access_tokens::AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("member", vec![source.group_id.clone()], false, None)
        .expect("member token");
    let boundary = "cccc-cross-group-denied";
    let (status, payload) = post_as(
        &home,
        Some(&member.token),
        &source.group_id,
        boundary,
        multipart(boundary, &destination.group_id),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{payload}");
    assert_eq!(blob_count(&groups, &source.group_id), before);

    let _ = cccc_client::DaemonClient::new(home)
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
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

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use http_body_util::BodyExt;
use serde_json::Map;
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;

#[tokio::test]
async fn branding_upload_rolls_back_staged_asset_when_daemon_commit_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::create_dir_all(home.root().join("state/web_branding")).expect("asset directory");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    std::fs::set_permissions(home.root(), std::fs::Permissions::from_mode(0o555))
        .expect("make settings directory read-only");
    let boundary = "branding-rollback";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\npng-bytes\r\n--{boundary}--\r\n"
    );
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        cccc_web::app(home.clone()).oneshot(
            Request::post("/api/v1/branding/assets/logo_icon")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        ),
    )
    .await;
    std::fs::set_permissions(home.root(), std::fs::Permissions::from_mode(0o755))
        .expect("restore settings directory");
    let _ = cccc_client::DaemonClient::new(home.clone())
        .call(&DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
    let response = response
        .expect("branding request timeout")
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: serde_json::Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_eq!(payload["ok"], false);
    let asset_root = home.root().join("state").join("web_branding");
    assert!(
        !asset_root.exists()
            || std::fs::read_dir(asset_root)
                .expect("asset directory")
                .next()
                .is_none(),
        "failed commit exposed an orphan branding asset"
    );
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

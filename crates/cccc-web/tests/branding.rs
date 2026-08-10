#![cfg(unix)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use tower::ServiceExt;

#[tokio::test]
async fn branding_is_public_to_read_and_admin_only_to_mutate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("home");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_address(&home).await;
    let app = cccc_web::app(home.clone());

    let public = app
        .clone()
        .oneshot(
            Request::get("/api/v1/branding")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("branding");
    assert_eq!(public.status(), StatusCode::OK);
    assert_eq!(
        json(public).await["result"]["branding"]["product_name"],
        "CCCC"
    );

    let default_manifest = app
        .clone()
        .oneshot(
            Request::get("/ui/manifest.webmanifest")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("manifest");
    assert_eq!(default_manifest.status(), StatusCode::OK);
    assert_eq!(
        default_manifest.headers()[header::CONTENT_TYPE],
        "application/manifest+json"
    );
    assert_eq!(
        default_manifest.headers()[header::CACHE_CONTROL],
        "no-cache"
    );
    assert_eq!(
        json(default_manifest).await["icons"][0]["src"],
        "/ui/logo.svg"
    );

    let boundary = "branding-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\npng-bytes\r\n--{boundary}--\r\n"
    );
    let denied = app
        .clone()
        .oneshot(
            Request::post("/api/v1/branding/assets/logo_icon")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body.clone()))
                .expect("request"),
        )
        .await
        .expect("denied");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let uploaded = app
        .clone()
        .oneshot(
            Request::post("/api/v1/branding/assets/logo_icon")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("uploaded");
    assert_eq!(uploaded.status(), StatusCode::OK);
    let uploaded = json(uploaded).await;
    assert_eq!(uploaded["result"]["branding"]["has_custom_logo_icon"], true);

    let custom_manifest = app
        .clone()
        .oneshot(
            Request::get("/ui/manifest.webmanifest")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("manifest");
    let custom_manifest = json(custom_manifest).await;
    assert_eq!(custom_manifest["icons"][0]["sizes"], "any");
    assert!(
        custom_manifest["icons"][0]["src"]
            .as_str()
            .is_some_and(|value| value.starts_with("/api/v1/branding/assets/logo_icon?v="))
    );

    let asset = app
        .clone()
        .oneshot(
            Request::get("/api/v1/branding/assets/logo_icon")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("asset");
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.into_body().collect().await.expect("body").to_bytes(),
        "png-bytes"
    );

    let deleted = app
        .clone()
        .oneshot(
            Request::delete("/api/v1/branding/assets/logo_icon")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("deleted");
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(
        json(deleted).await["result"]["branding"]["has_custom_logo_icon"],
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

async fn json(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json")
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

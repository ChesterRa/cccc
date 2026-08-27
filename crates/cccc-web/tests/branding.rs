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
    assert_eq!(custom_manifest["icons"][0]["type"], "image/svg+xml");
    assert_eq!(custom_manifest["icons"][0]["purpose"], "any");
    assert!(
        custom_manifest["icons"][0]["src"]
            .as_str()
            .is_some_and(|value| value.starts_with("/pwa-icon.svg?v="))
    );
    assert_eq!(custom_manifest["icons"][1]["purpose"], "maskable");

    for path in ["/pwa-icon.svg", "/pwa-icon-maskable.svg"] {
        let icon = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("PWA icon");
        assert_eq!(icon.status(), StatusCode::OK);
        assert_eq!(icon.headers()[header::CONTENT_TYPE], "image/svg+xml");
        let body = icon.into_body().collect().await.expect("body").to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("data:image/png;base64,cG5nLWJ5dGVz"));
    }

    let apple = app
        .clone()
        .oneshot(
            Request::get("/apple-touch-icon.png")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("Apple icon");
    assert_eq!(apple.status(), StatusCode::TEMPORARY_REDIRECT);
    assert!(
        apple.headers()[header::LOCATION]
            .to_str()
            .expect("location")
            .starts_with("/api/v1/branding/assets/logo_icon?v=")
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

    let asset_head = app
        .clone()
        .oneshot(
            Request::head("/api/v1/branding/assets/logo_icon")
                .header(header::AUTHORIZATION, "Bearer invalid-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("asset HEAD");
    assert_eq!(asset_head.status(), StatusCode::OK);
    assert_eq!(asset_head.headers()[header::CONTENT_LENGTH], "9");
    assert!(
        asset_head
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .is_empty()
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

#[tokio::test]
async fn public_branding_errors_do_not_expose_home_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("private-rust-home")).expect("home");
    home.initialize().expect("home");
    let mut settings = cccc_core::settings::GlobalSettings::default();
    settings.branding.insert(
        "logo_icon_asset_path".into(),
        Value::String("state/web_branding/private-logo.png".into()),
    );
    cccc_core::settings::save(&home, &settings).expect("settings");
    let app = cccc_web::app(home.clone());

    for (path, message) in [
        ("/pwa-icon.svg", "branding icon unavailable"),
        (
            "/api/v1/branding/assets/logo_icon",
            "custom branding asset not found",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("branding response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = json(response).await;
        assert_eq!(body["error"]["message"], message);
        assert!(
            !body
                .to_string()
                .contains(&home.root().to_string_lossy()[..])
        );
    }
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

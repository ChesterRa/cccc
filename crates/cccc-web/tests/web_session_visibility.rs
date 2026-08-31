use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn scoped_session_receives_saved_runtime_visibility_without_admin_counts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::settings::save(
        &home,
        &cccc_core::settings::GlobalSettings {
            observability: json!({
                "runtime_visibility": {
                    "peer_runtime": "hidden",
                    "assistant_runtime": "visible"
                }
            })
            .as_object()
            .cloned()
            .expect("observability"),
            ..Default::default()
        },
    )
    .expect("save settings");
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    tokens
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let scoped = tokens
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("scoped token");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session")
                .header(header::AUTHORIZATION, format!("Bearer {}", scoped.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let session = &payload["result"]["web_access_session"];
    assert_eq!(session["access_token_count"], 0);
    assert_eq!(
        session["runtime_visibility"],
        json!({"peer_runtime":"hidden","assistant_runtime":"visible"})
    );
}

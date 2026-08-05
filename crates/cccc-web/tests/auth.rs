use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout, ledger};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn first_admin_token_bootstraps_login_cookie() {
    let (_temp, home) = home();
    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/access-tokens")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"user_id":"admin","is_admin":true}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("cccc_access_token=acc_"))
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(
        payload["result"]["access_token"]["token"]
            .as_str()
            .is_some_and(|token| token.starts_with("acc_"))
    );
}

#[tokio::test]
async fn configured_tokens_reject_anonymous_api_requests() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn scoped_token_cannot_open_another_group() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups/g_denied/actors")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn scoped_token_cannot_delegate_into_another_group() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/groups/g_allowed/delegate_contact")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"dst_group_id":"g_denied","text":"hello"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn scoped_token_cannot_access_legacy_profiles_or_provider_credentials() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    for path in [
        "/api/v1/actor_profiles",
        "/api/v1/space/providers/notebooklm/credential",
    ] {
        let response = cccc_web::app(home.clone())
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn scoped_token_cannot_access_global_management_routes() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    for (method, path) in [
        ("GET", "/api/v1/remote_access"),
        ("POST", "/api/v1/remote_access/start"),
        ("GET", "/api/v1/debug/tail_logs"),
        ("POST", "/api/v1/debug/clear_logs"),
        ("GET", "/api/v1/capabilities/allowlist"),
        ("POST", "/api/v1/capabilities/allowlist/validate"),
        ("POST", "/api/v1/capabilities/block"),
    ] {
        let response = cccc_web::app(home.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }
}

#[tokio::test]
async fn scoped_query_token_global_stream_only_exposes_allowed_groups() {
    let (_temp, home) = home();
    let groups = GroupStore::new(home.clone()).expect("groups");
    let allowed = groups.create("allowed", "").expect("allowed group");
    let denied = groups.create("denied", "").expect("denied group");
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec![allowed.group_id.clone()], false, None)
        .expect("token");

    let response = cccc_web::app(home.clone())
        .oneshot(
            Request::get(format!("/api/v1/events/stream?token={}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();

    let mut denied_event = cccc_contracts::Event::new("chat.message", &denied.group_id);
    denied_event
        .data
        .insert("text".into(), Value::String("DENIED_GROUP_SECRET".into()));
    ledger::append(
        &groups.ledger_path(&denied.group_id).expect("denied ledger"),
        &denied_event,
    )
    .expect("append denied event");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut allowed_event = cccc_contracts::Event::new("group.updated", &allowed.group_id);
    allowed_event
        .data
        .insert("text".into(), Value::String("ALLOWED_GROUP_SECRET".into()));
    ledger::append(
        &groups
            .ledger_path(&allowed.group_id)
            .expect("allowed ledger"),
        &allowed_event,
    )
    .expect("append allowed event");

    let mut received = String::new();
    while !received.contains(&allowed_event.id) {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), body.next())
            .await
            .expect("global SSE timeout")
            .expect("global SSE body ended")
            .expect("global SSE chunk");
        received.push_str(std::str::from_utf8(&chunk).expect("SSE is UTF-8"));
    }

    assert!(received.contains(&allowed.group_id));
    assert!(!received.contains(&denied.group_id));
    assert!(!received.contains("DENIED_GROUP_SECRET"));
    assert!(!received.contains("ALLOWED_GROUP_SECRET"));
}

#[tokio::test]
async fn scoped_token_cannot_snapshot_a_query_selected_group() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("token");
    let app = cccc_web::app(home);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/v1/debug/snapshot?group%5Fid=g%5Fdenied")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .oneshot(
            Request::get("/api/v1/debug/snapshot?group_id=g_allowed&group_id=g_denied")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn legacy_flat_token_document_keeps_authentication_enabled() {
    let (_temp, home) = home();
    std::fs::write(
        home.root().join("access_tokens.yaml"),
        concat!(
            "legacy-flat-token:\n",
            "  user_id: legacy-user\n",
            "  allowed_groups: []\n",
            "  is_admin: true\n",
            "  created_at: '2026-01-01T00:00:00Z'\n",
            "  updated_at: '2026-01-01T00:00:00Z'\n",
        ),
    )
    .expect("fixture");

    let anonymous = cccc_web::app(home.clone())
        .oneshot(
            Request::get("/api/v1/groups")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let session = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session?token=legacy-flat-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = session
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["web_access_session"]["current_browser_signed_in"],
        true
    );
}

#[tokio::test]
async fn encoded_custom_token_authenticates_event_source_queries() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, Some("token;+ 含"))
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session?token=token%3B%2B%20%E5%90%AB")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["web_access_session"]["current_browser_signed_in"],
        true
    );
}

#[tokio::test]
async fn cannot_delete_the_last_admin_while_scoped_tokens_remain() {
    let (_temp, home) = home();
    let store = AccessTokenStore::new(home.clone()).expect("store");
    let admin = store
        .create("admin", Vec::new(), true, None)
        .expect("admin");
    store
        .create("member", vec!["g_allowed".into()], false, None)
        .expect("member");
    let response = cccc_web::app(home)
        .oneshot(
            Request::delete(format!("/api/v1/access-tokens/{}", admin.token_id()))
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    (temp, home)
}

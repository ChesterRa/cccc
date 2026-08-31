use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout, ledger};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[path = "auth/local_passwordless.rs"]
mod local_passwordless;

#[tokio::test]
async fn ready_identifies_the_web_instance_without_a_daemon_round_trip() {
    let (_temp, home) = home();
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/ready")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["result"]["web"], "ready");
    assert!(payload["result"]["runtime_id"].is_null());
}

#[tokio::test]
async fn first_admin_token_bootstraps_login_cookie() {
    let (_temp, home) = home();
    let bootstrap_path = cccc_core::web_bootstrap::ensure_web_bootstrap_token(&home)
        .expect("bootstrap")
        .expect("bootstrap path");
    let bootstrap_token = std::fs::read_to_string(bootstrap_path).expect("bootstrap token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/api/v1/access-tokens")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"user_id":"admin","is_admin":true,"bootstrap_token":"{}"}}"#,
                    bootstrap_token.trim()
                )))
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
async fn empty_token_store_rejects_protected_routes_and_wrong_bootstrap_code() {
    let (_temp, home) = home();
    let app = cccc_web::app(home);
    let protected = app
        .clone()
        .oneshot(
            Request::get("/api/v1/fs/list?path=~")
                .header(header::HOST, "cccc.example")
                .header("x-forwarded-for", "203.0.113.10")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(protected.status(), StatusCode::UNAUTHORIZED);

    let bootstrap = app
        .oneshot(
            Request::post("/api/v1/access-tokens")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"user_id":"attacker","is_admin":true,"bootstrap_token":"wrong"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(bootstrap.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_token_does_not_authenticate_or_set_a_cookie() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get(format!("/api/v1/web_access/session?token={}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["result"]["web_access_session"]["current_browser_signed_in"],
        false
    );
}

#[tokio::test]
async fn authorization_header_bootstraps_cookie_without_a_query_secret() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
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
}

#[tokio::test]
async fn cookie_authenticated_writes_require_an_allowed_origin() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let app = cccc_web::app(home);

    for origin in [None, Some("https://evil.example")] {
        let mut request = Request::post("/api/v1/web_access/logout")
            .header(header::HOST, "cccc.example")
            .header(header::COOKIE, format!("cccc_access_token={}", token.token));
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
    }

    let same_origin = app
        .clone()
        .oneshot(
            Request::post("/api/v1/web_access/logout")
                .header(header::HOST, "cccc.example")
                .header(header::ORIGIN, "http://cccc.example")
                .header(header::COOKIE, format!("cccc_access_token={}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(same_origin.status(), StatusCode::OK);

    let bearer_without_origin = app
        .oneshot(
            Request::post("/api/v1/web_access/logout")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(bearer_without_origin.status(), StatusCode::OK);
}

#[tokio::test]
async fn incidental_web_cookie_does_not_block_a_public_connector_request() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::post("/mcp/web-model/missing")
                .header(header::COOKIE, format!("cccc_access_token={}", token.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("connector token required"), "{text}");
    assert!(!text.contains("csrf_origin_invalid"), "{text}");
}

#[tokio::test]
async fn web_login_exchange_is_origin_bound_and_one_time() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let grant =
        cccc_core::web_login_grants::issue(&home, "http://reach.example", &token.token_id(), 120)
            .expect("grant");
    let app = cccc_web::app(home);
    let path = format!("/api/v1/web_access/exchange?code={}", grant.code);

    let wrong_origin = app
        .clone()
        .oneshot(
            Request::get(&path)
                .header(header::HOST, "other.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(wrong_origin.status(), StatusCode::UNAUTHORIZED);

    let accepted = app
        .clone()
        .oneshot(
            Request::get(&path)
                .header(header::HOST, "reach.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        accepted
            .headers()
            .get(header::LOCATION)
            .expect("redirect location"),
        "/ui/"
    );
    assert_eq!(
        accepted
            .headers()
            .get(header::CACHE_CONTROL)
            .expect("cache control"),
        "no-store"
    );
    assert!(
        accepted
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.contains(&token.token)
                    && value.contains("HttpOnly")
                    && value.contains("SameSite=Lax")
                    && !value.contains("Secure")
            })
    );

    let replay = app
        .oneshot(
            Request::get(path)
                .header(header::HOST, "reach.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn query_token_cannot_replace_a_valid_authentication_cookie() {
    let (_temp, home) = home();
    let store = AccessTokenStore::new(home.clone()).expect("store");
    let stale = store
        .create("stale-admin", Vec::new(), true, None)
        .expect("stale token");
    let current = store
        .create("current-admin", Vec::new(), true, None)
        .expect("current token");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get(format!(
                "/api/v1/web_access/session?token={}",
                current.token
            ))
            .header(header::COOKIE, format!("cccc_access_token={}", stale.token))
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
    assert_eq!(
        payload["result"]["web_access_session"]["user_id"],
        stale.user_id
    );
}

#[tokio::test]
async fn protected_routes_ignore_legacy_query_tokens_and_use_the_valid_cookie() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups?token=invalid")
                .header(header::COOKIE, format!("cccc_access_token={}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
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
        ("GET", "/api/v1/membership"),
        ("POST", "/api/v1/membership/login"),
        ("POST", "/api/v1/membership/login/poll"),
        ("POST", "/api/v1/membership/logout"),
        ("POST", "/api/v1/membership/reach/on"),
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
async fn scoped_cookie_global_stream_only_exposes_allowed_groups() {
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
            Request::get("/api/v1/events/stream")
                .header(header::COOKIE, format!("cccc_access_token={}", token.token))
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
async fn long_lived_query_token_is_rejected_for_event_streams() {
    let (_temp, home) = home();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get(format!("/api/v1/events/stream?token={}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
            Request::get("/api/v1/web_access/session")
                .header(header::AUTHORIZATION, "Bearer legacy-flat-token")
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
async fn malformed_token_document_fails_closed_without_leaking_parser_details() {
    let (_temp, home) = home();
    std::fs::write(home.root().join("access_tokens.yaml"), "tokens: [").expect("malformed fixture");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/access-tokens")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["error"]["code"], "auth_store_error");
    assert_eq!(
        payload["error"]["message"],
        "access token store is unavailable"
    );
    assert!(!String::from_utf8_lossy(&body).contains("expected"));
}

#[tokio::test]
async fn unavailable_daemon_response_does_not_expose_transport_paths() {
    let (_temp, home) = home();
    let home_path = home.root().display().to_string();
    let token = AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, None)
        .expect("token");

    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/groups")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["error"]["code"], "daemon_unavailable");
    assert_eq!(payload["error"]["message"], "CCCC daemon unavailable");
    assert!(!String::from_utf8_lossy(&body).contains(&home_path));
}

#[tokio::test]
async fn bearer_safe_custom_token_authenticates_with_a_header() {
    let (_temp, home) = home();
    AccessTokenStore::new(home.clone())
        .expect("store")
        .create("admin", Vec::new(), true, Some("token+/_-."))
        .expect("token");
    let response = cccc_web::app(home)
        .oneshot(
            Request::get("/api/v1/web_access/session")
                .header(header::AUTHORIZATION, "Bearer token+/_-.")
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
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["error"]["code"], "last_admin_required");
    assert!(
        store
            .lookup(&admin.token)
            .expect("lookup")
            .is_some_and(|token| token.is_admin)
    );
}

#[tokio::test]
async fn cannot_demote_the_last_admin_while_scoped_tokens_remain() {
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
            Request::patch(format!("/api/v1/access-tokens/{}", admin.token_id()))
                .header(header::AUTHORIZATION, format!("Bearer {}", admin.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"is_admin":false,"allowed_groups":["g_allowed"]}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["error"]["code"], "last_admin_required");
    assert!(
        store
            .lookup(&admin.token)
            .expect("lookup")
            .is_some_and(|token| token.is_admin)
    );
}

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    (temp, home)
}

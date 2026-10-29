use super::super::store::endpoint_hash;
use crate::WebMode;
use aws_lc_rs::agreement::{ECDH_P256, PrivateKey};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

const ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send/cccc-route-test";
const ORIGIN: &str = "http://127.0.0.1:8848";

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    router: Router,
    state: crate::AppState,
    admin: String,
    member: String,
    daemon: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.daemon.abort();
    }
}

/// An isolated Home with its own daemon, an administrator and a member
/// token. No test here writes a message to the user, so nothing is sent.
async fn fixture(mode: WebMode) -> Fixture {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let admin = tokens
        .create("admin", Vec::new(), true, None)
        .expect("admin")
        .token;
    let member = tokens
        .create("member", vec!["g_missing".into()], false, None)
        .expect("member")
        .token;
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let (router, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown,
        mode,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 8848,
        },
        "push-route-fixture".into(),
    );
    let daemon = tokio::spawn({
        let home = home.clone();
        async move {
            cccc_daemon::run(home).await.expect("isolated daemon");
        }
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while state
            .client
            .call(&DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("daemon ready");
    Fixture {
        _temp: temp,
        home,
        router,
        state,
        admin,
        member,
        daemon,
    }
}

impl Fixture {
    async fn call(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.request(method, path, token, body, Some(ORIGIN)).await
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
        origin: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "127.0.0.1:8848")
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn subscription(endpoint: &str, server_key: &str) -> Value {
    let browser = PrivateKey::generate(&ECDH_P256).expect("browser key");
    json!({
        "endpoint": endpoint,
        "expirationTime": null,
        "keys": {
            "p256dh": URL_SAFE_NO_PAD.encode(browser.compute_public_key().expect("public").as_ref()),
            "auth": URL_SAFE_NO_PAD.encode([9_u8; 16]),
        },
        "application_server_key": server_key,
    })
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn subscription_keys_are_bounded_canonical_and_rebinding_is_stable() {
    let f = fixture(WebMode::Normal).await;
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.admin, None).await;
    let key = body["result"]["vapid_public_key"].as_str().expect("key");
    let valid = subscription(ENDPOINT, key);
    let p256dh = valid["keys"]["p256dh"].as_str().expect("p256dh").to_owned();
    let auth = valid["keys"]["auth"].as_str().expect("auth").to_owned();
    let mut padded = valid.clone();
    padded["keys"]["p256dh"] = json!(format!("{p256dh}{}", "=".repeat(100)));
    padded["keys"]["auth"] = json!(format!("{auth}{}", "=".repeat(100)));
    let (status, body) = f
        .call("PUT", "/api/v1/push/subscription", &f.admin, Some(padded))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let store = super::super::store::PushStore::new(f.home.clone());
    let (_, items) = store.snapshot().expect("read").expect("key");
    assert_eq!(items[0].p256dh, p256dh);
    assert_eq!(items[0].auth, auth);
    let binding = items[0].binding.clone();
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(valid.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, items) = store.snapshot().expect("read").expect("key");
    assert_eq!(items[0].binding, binding, "same keys keep their place");

    let mut oversized = valid;
    oversized["keys"]["auth"] = json!(format!("{auth}{}", "=".repeat(8192)));
    let (status, _) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(oversized),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (_, items) = store.snapshot().expect("read").expect("key");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].binding, binding);
}

#[tokio::test]
async fn a_browser_binds_and_unbinds_only_for_its_own_principal() {
    let f = fixture(WebMode::Normal).await;
    let (status, body) = f.call("GET", "/api/v1/push/config", &f.admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"]["vapid_public_key"], json!(null));
    assert_eq!(body["result"]["subscriptions"], json!([]));
    assert!(
        ["starting", "here"].contains(&body["result"]["sender"].as_str().unwrap_or_default()),
        "{body}"
    );
    let response = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/push/config")
                .header(header::HOST, "127.0.0.1:8848")
                .header(header::AUTHORIZATION, format!("Bearer {}", f.admin))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");

    let (status, body) = f.call("POST", "/api/v1/push/key", &f.member, None).await;
    assert_eq!(status, StatusCode::OK);
    let key = body["result"]["vapid_public_key"]
        .as_str()
        .expect("key")
        .to_owned();
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.admin, None).await;
    assert_eq!(body["result"]["vapid_public_key"], json!(key), "made once");

    let valid = subscription(ENDPOINT, &key);
    let (status, body) = f
        .request(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(valid.clone()),
            None,
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "push_origin_invalid")
    );
    let (status, body) = f
        .request(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(valid.clone()),
            Some("https://elsewhere.example"),
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "push_origin_invalid")
    );

    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(subscription("https://example.com/push", &key)),
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "push_service_unsupported")
    );
    let mut bad_keys = valid.clone();
    bad_keys["keys"]["p256dh"] = json!("AAAA");
    let (status, body) = f
        .call("PUT", "/api/v1/push/subscription", &f.admin, Some(bad_keys))
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "push_subscription_invalid")
    );
    let other_key = URL_SAFE_NO_PAD.encode(
        PrivateKey::generate(&ECDH_P256)
            .expect("key")
            .compute_public_key()
            .expect("public")
            .as_ref(),
    );
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(subscription(ENDPOINT, &other_key)),
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "push_key_mismatch")
    );

    let (status, body) = f
        .call("PUT", "/api/v1/push/subscription", &f.admin, Some(valid))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"], json!({"subscribed": true}));
    let (_, body) = f.call("GET", "/api/v1/push/config", &f.admin, None).await;
    assert_eq!(
        body["result"]["subscriptions"],
        json!([endpoint_hash(ENDPOINT)])
    );
    let serialized = body.to_string();
    assert!(
        !serialized.contains("cccc-route-test"),
        "endpoints stay private"
    );

    // Another principal sees none of it and cannot remove it.
    let (_, body) = f.call("GET", "/api/v1/push/config", &f.member, None).await;
    assert_eq!(body["result"]["subscriptions"], json!([]));
    let (status, body) = f
        .call(
            "DELETE",
            "/api/v1/push/subscription",
            &f.member,
            Some(json!({"endpoint": ENDPOINT})),
        )
        .await;
    assert_eq!(
        (status, &body["result"]),
        (StatusCode::OK, &json!({"removed": false}))
    );
    let (_, body) = f
        .call(
            "DELETE",
            "/api/v1/push/subscription",
            &f.admin,
            Some(json!({"endpoint": ENDPOINT})),
        )
        .await;
    assert_eq!(body["result"], json!({"removed": true}));
    let (_, body) = f.call("GET", "/api/v1/push/config", &f.admin, None).await;
    assert_eq!(body["result"]["subscriptions"], json!([]));
}

#[tokio::test]
async fn a_member_binds_with_a_deleted_group_still_in_its_token() {
    let f = fixture(WebMode::Normal).await;
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.member, None).await;
    let key = body["result"]["vapid_public_key"].as_str().expect("key");
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.member,
            Some(subscription(ENDPOINT, key)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = f.call("GET", "/api/v1/push/config", &f.member, None).await;
    assert_eq!(
        body["result"]["subscriptions"],
        json!([endpoint_hash(ENDPOINT)])
    );
}

#[tokio::test]
async fn an_exhibit_neither_shows_nor_changes_notification_settings() {
    let f = fixture(WebMode::Exhibit).await;
    for (method, path) in [
        ("GET", "/api/v1/push/config"),
        ("POST", "/api/v1/push/key"),
        ("PUT", "/api/v1/push/subscription"),
        ("DELETE", "/api/v1/push/subscription"),
    ] {
        let (status, body) = f.call(method, path, &f.admin, Some(json!({}))).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::FORBIDDEN, "read_only"),
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn the_push_worker_scope_never_falls_back_to_the_app() {
    for path in ["/ui/push/", "/ui/push/missing", "/ui/push/nested/page"] {
        let response =
            crate::static_asset(axum::http::Method::GET, path.parse().expect("URI")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_binding_starts_after_the_newest_event_of_each_group() {
    let f = fixture(WebMode::Normal).await;
    let groups = cccc_core::GroupStore::new(f.home.clone()).expect("groups");
    let busy = groups.create("busy", "").expect("group").group_id;
    let quiet = groups.create("quiet", "").expect("group").group_id;
    let newest = cccc_contracts::Event::new("chat.message", &busy);
    cccc_core::ledger::append(&groups.ledger_path(&busy).expect("path"), &newest).expect("append");
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.admin, None).await;
    let key = body["result"]["vapid_public_key"].as_str().expect("key");
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(subscription(ENDPOINT, key)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, subscriptions) = super::super::store::PushStore::new(f.home.clone())
        .snapshot()
        .expect("read")
        .expect("key");
    let since = &subscriptions[0].since;
    assert_eq!(since.get(&busy), Some(&newest.id));
    assert_eq!(
        since.get(&quiet).map(String::as_str),
        Some(""),
        "nothing written yet"
    );

    // A token reads only the Groups it may see; another one's broken ledger
    // does not stop it.
    let ledger = groups.ledger_path(&busy).expect("path");
    std::fs::remove_file(&ledger).expect("remove");
    std::fs::create_dir(&ledger).expect("break");
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.member, None).await;
    let key = body["result"]["vapid_public_key"].as_str().expect("key");
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.member,
            Some(subscription(
                "https://fcm.googleapis.com/fcm/send/member",
                key,
            )),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, subscriptions) = super::super::store::PushStore::new(f.home.clone())
        .snapshot()
        .expect("read")
        .expect("key");
    let member = subscriptions
        .iter()
        .find(|item| item.endpoint.ends_with("/member"))
        .expect("member");
    assert!(member.since.is_empty(), "{:?}", member.since);
}

#[tokio::test]
async fn nothing_is_bound_where_nothing_can_send() {
    let f = fixture(WebMode::Normal).await;
    f.state
        .push
        .set(super::super::notifier::SenderState::Unavailable);
    let (_, body) = f.call("GET", "/api/v1/push/config", &f.admin, None).await;
    assert_eq!(body["result"]["sender"], json!("unavailable"));
    let (_, body) = f.call("POST", "/api/v1/push/key", &f.admin, None).await;
    let key = body["result"]["vapid_public_key"].as_str().expect("key");
    let (status, body) = f
        .call(
            "PUT",
            "/api/v1/push/subscription",
            &f.admin,
            Some(subscription(ENDPOINT, key)),
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::SERVICE_UNAVAILABLE, "push_sender_unavailable")
    );
}

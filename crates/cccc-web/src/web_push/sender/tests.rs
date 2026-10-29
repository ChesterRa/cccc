use super::*;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use std::sync::Arc;

#[test]
fn push_service_answers_are_classified() {
    for status in [200, 201, 202] {
        assert_eq!(classify(status), Outcome::Accepted);
    }
    for status in [404, 410] {
        assert_eq!(classify(status), Outcome::Gone);
    }
    for status in [408, 429, 500, 502, 503] {
        assert_eq!(classify(status), Outcome::Retry);
    }
    // A key or token problem is ours: the subscription is kept.
    for status in [301, 302, 400, 401, 403, 413] {
        assert_eq!(classify(status), Outcome::Refused);
    }
}

#[test]
fn a_vapid_token_is_reused_per_push_service() {
    let key = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("key");
    let sender = Sender::new().expect("sender");
    let fcm = sender
        .authorization(&key, "https://fcm.googleapis.com")
        .expect("token");
    assert_eq!(
        sender.authorization(&key, "https://fcm.googleapis.com"),
        Some(fcm.clone())
    );
    let apple = sender
        .authorization(&key, "https://web.push.apple.com")
        .expect("token");
    assert_ne!(fcm, apple);
    assert!(fcm.ends_with(&format!(", k={}", key.public_key())));
    // A different key never reuses another key's token.
    let other = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("key");
    let renewed = sender
        .authorization(&other, "https://fcm.googleapis.com")
        .expect("token");
    assert!(renewed.ends_with(other.public_key()));
}

type Seen = Arc<std::sync::Mutex<Vec<(HeaderMap, usize)>>>;

/// A push service on loopback that records what it receives and answers
/// with `status`, or redirects to `/elsewhere` when `status` is 307.
async fn push_service(status: StatusCode) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let record = Arc::clone(&seen);
    let elsewhere = Arc::clone(&seen);
    let app = axum::Router::new()
        .route(
            "/push",
            post(move |headers: HeaderMap, body: axum::body::Bytes| {
                let record = Arc::clone(&record);
                async move {
                    record.lock().expect("seen").push((headers, body.len()));
                    if status == StatusCode::TEMPORARY_REDIRECT {
                        return (status, [("location", "/elsewhere")]).into_response();
                    }
                    status.into_response()
                }
            }),
        )
        .route(
            "/elsewhere",
            post(move || {
                let elsewhere = Arc::clone(&elsewhere);
                async move {
                    elsewhere
                        .lock()
                        .expect("seen")
                        .push((HeaderMap::new(), usize::MAX));
                    StatusCode::CREATED
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("push service");
    });
    (format!("http://{address}/push"), seen)
}

use axum::response::IntoResponse;

#[tokio::test]
async fn a_notification_is_posted_encrypted_with_its_headers() {
    let (url, seen) = push_service(StatusCode::CREATED).await;
    let sender = Sender::new().expect("sender");
    let outcome = sender
        .post(
            Url::parse(&url).expect("url"),
            "vapid t=a.b.c, k=key".into(),
            vec![1, 2, 3],
        )
        .await;
    assert_eq!(outcome, Outcome::Accepted);
    let seen = seen.lock().expect("seen");
    let (headers, length) = &seen[0];
    assert_eq!(*length, 3);
    for (name, value) in [
        ("ttl", "86400"),
        ("urgency", "normal"),
        ("content-encoding", "aes128gcm"),
        ("content-type", "application/octet-stream"),
        ("authorization", "vapid t=a.b.c, k=key"),
    ] {
        assert_eq!(
            headers.get(name).and_then(|value| value.to_str().ok()),
            Some(value),
            "{name}"
        );
    }
    assert!(headers.get("topic").is_none());
}

#[tokio::test]
async fn redirects_are_not_followed_and_answers_decide_what_happens_next() {
    for (status, expected) in [
        (StatusCode::TEMPORARY_REDIRECT, Outcome::Refused),
        (StatusCode::GONE, Outcome::Gone),
        (StatusCode::NOT_FOUND, Outcome::Gone),
        (StatusCode::TOO_MANY_REQUESTS, Outcome::Retry),
        (StatusCode::SERVICE_UNAVAILABLE, Outcome::Retry),
        (StatusCode::FORBIDDEN, Outcome::Refused),
    ] {
        let (url, seen) = push_service(status).await;
        let sender = Sender::new().expect("sender");
        let outcome = sender
            .post(Url::parse(&url).expect("url"), "vapid".into(), Vec::new())
            .await;
        assert_eq!(outcome, expected, "{status}");
        assert_eq!(seen.lock().expect("seen").len(), 1, "{status} was followed");
    }
    // Nothing listening is worth another try.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    let closed = format!("http://{}/push", listener.local_addr().expect("address"));
    drop(listener);
    let outcome = Sender::new()
        .expect("sender")
        .post(
            Url::parse(&closed).expect("url"),
            "vapid".into(),
            Vec::new(),
        )
        .await;
    assert_eq!(outcome, Outcome::Retry);
}

#[tokio::test]
async fn an_endpoint_outside_the_push_services_is_never_contacted() {
    let key = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("key");
    let (url, seen) = push_service(StatusCode::CREATED).await;
    let subscription = Subscription {
        endpoint: url,
        binding: "b".into(),
        p256dh: String::new(),
        auth: String::new(),
        application_server_key: key.public_key().into(),
        owner: super::super::store::Owner::Local,
        origin: "https://cccc.example".into(),
        created_at: String::new(),
        updated_at: String::new(),
        since: Default::default(),
    };
    let outcome = Sender::new()
        .expect("sender")
        .send(&key, &subscription, b"{}")
        .await;
    assert_eq!(outcome, Outcome::Gone);
    assert!(seen.lock().expect("seen").is_empty());
}

/// A subscription to `endpoint` with keys a browser could have made.
fn subscription(endpoint: &str, key: &VapidKey) -> Subscription {
    use aws_lc_rs::agreement::{ECDH_P256, PrivateKey};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let browser = PrivateKey::generate(&ECDH_P256).expect("browser");
    Subscription {
        endpoint: endpoint.into(),
        binding: "b".into(),
        p256dh: URL_SAFE_NO_PAD.encode(browser.compute_public_key().expect("public").as_ref()),
        auth: URL_SAFE_NO_PAD.encode([3_u8; 16]),
        application_server_key: key.public_key().into(),
        owner: super::super::store::Owner::Local,
        origin: "https://cccc.example".into(),
        created_at: String::new(),
        updated_at: String::new(),
        since: Default::default(),
    }
}

/// A push service on loopback that answers `answers` in turn, repeating the
/// last, and records the Authorization header of each request.
async fn answering(answers: Vec<StatusCode>) -> (Url, Arc<std::sync::Mutex<Vec<String>>>) {
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let record = Arc::clone(&seen);
    let app = axum::Router::new().route(
        "/push",
        post(move |headers: HeaderMap| {
            let record = Arc::clone(&record);
            let answers = answers.clone();
            async move {
                let mut seen = record.lock().expect("seen");
                seen.push(
                    headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned(),
                );
                answers[(seen.len() - 1).min(answers.len() - 1)]
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("push service");
    });
    (
        Url::parse(&format!("http://{address}/push")).expect("url"),
        seen,
    )
}

#[tokio::test]
async fn a_rejected_token_is_not_reused_and_a_fresh_one_is_tried_once() {
    let key = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("key");
    let stale = format!("vapid t=stale, k={}", key.public_key());
    for (answers, expected, tries) in [
        (
            vec![StatusCode::UNAUTHORIZED, StatusCode::CREATED],
            Outcome::Accepted,
            2,
        ),
        (vec![StatusCode::FORBIDDEN], Outcome::Refused, 2),
        (vec![StatusCode::BAD_REQUEST], Outcome::Refused, 1),
    ] {
        let (url, seen) = answering(answers.clone()).await;
        let mut sender = Sender::new().expect("sender");
        let subscription = subscription("https://fcm.googleapis.com/fcm/send/x", &key);
        sender
            .stand_ins
            .insert(subscription.endpoint.clone(), url.clone());
        // A token cached from before, as one signed while the clock was wrong.
        sender.tokens.lock().expect("tokens").insert(
            "https://fcm.googleapis.com".into(),
            (stale.clone(), u64::MAX),
        );
        let outcome = sender.send(&key, &subscription, b"{}").await;
        assert_eq!(outcome, expected, "{answers:?}");
        let seen = seen.lock().expect("seen").clone();
        assert_eq!(seen.len(), tries, "{answers:?}");
        assert_eq!(seen[0], stale);
        if tries == 2 {
            assert_ne!(seen[1], stale);
            assert!(seen[1].ends_with(key.public_key()));
        }
    }
}

#[tokio::test]
async fn a_test_sender_does_not_post_to_a_push_service_it_was_not_given() {
    let key = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("key");
    let subscription = subscription("https://fcm.googleapis.com/fcm/send/unmapped", &key);
    let outcome = Sender::new()
        .expect("sender")
        .send(&key, &subscription, b"{}")
        .await;
    assert_eq!(outcome, Outcome::Retry);
}

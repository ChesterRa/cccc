//! Retired HTTP requests cannot issue authority; manual/native pairing stays intact.
use super::fixture::Fixture;
use super::join_support::{advertised_urls, call};
use axum::http::StatusCode;
use cccc_core::{access_tokens::AccessTokenStore, session_nodes};
use serde_json::{Value, json};

const BASE: &str = "/api/v1/session_nodes/join";

#[tokio::test]
async fn retired_browser_endpoints_are_side_effect_free_and_manual_native_pairing_still_works() {
    let f = Fixture::new().await;
    let tokens = AccessTokenStore::new(f.home.clone()).expect("isolated tokens");
    let admin = tokens
        .create("admin", vec![], true, Some("test-ticket-admin"))
        .expect("admin");
    let member = tokens
        .create("member", vec![], false, Some("test-ticket-member"))
        .expect("member");
    let app = crate::app(f.home.clone());
    let key = iroh::SecretKey::generate();
    let node = key.public().to_string();
    let path = f.home.root().join("state/session_nodes/state.json");
    let before = std::fs::read(&path).expect("existing authority");
    for (tail, body) in [
        (
            "requests",
            json!({"node_id":node,"name":"Laptop","device_code":"a".repeat(64)}),
        ),
        (
            "poll",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"device_code":"a".repeat(64)}),
        ),
    ] {
        let (status, result) = call(&app, "POST", &format!("{BASE}/{tail}"), None, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "session_node_join_retired");
        assert!(
            result["error"]["message"]
                .as_str()
                .expect("guidance")
                .contains("connect --ticket-stdin")
        );
        assert!(result.get("result").is_none());
    }
    let id = uuid::Uuid::new_v4();
    for (method, tail) in [
        ("GET", "pending".to_owned()),
        ("GET", format!("requests/{id}")),
        ("POST", format!("requests/{id}/approve")),
        ("POST", format!("requests/{id}/deny")),
    ] {
        let url = format!("{BASE}/{tail}");
        assert_eq!(
            call(&app, method, &url, Some(&admin.token), Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&app, method, &url, Some(&member.token), Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&app, method, &url, None, Value::Null).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert!(
        std::fs::read(&path).expect("authority unchanged") == before,
        "Retired requests must not change existing authority"
    );
    assert!(
        session_nodes::peers(&f.home)
            .expect("no enrollment")
            .is_empty()
    );
    let readonly = crate::app_with_mode(f.home.clone(), crate::WebMode::Exhibit);
    for tail in ["requests", "poll"] {
        assert_eq!(
            call(
                &readonly,
                "POST",
                &format!("{BASE}/{tail}"),
                None,
                Value::Null
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let (status, response) = call(
        &app,
        "POST",
        "/api/v1/session_nodes/invitations",
        Some(&admin.token),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ticket = crate::session_node::decode_ticket(
        response["result"]["ticket"]
            .as_str()
            .expect("manual ticket"),
    )
    .expect("native ticket");
    let endpoint = f.endpoint(key.clone(), Some(&ticket)).await;
    let (_, paired) = f.hello(&endpoint, Some(&ticket)).await;
    assert_eq!(paired["peer"]["node_id"], node);
    assert_eq!(paired["memberships"], json!([]));
    endpoint.close().await;
    let resumed = f.endpoint(key.clone(), None).await;
    let (_, connected) = f.hello(&resumed, None).await;
    assert_eq!(connected["peer"]["grant_id"], paired["peer"]["grant_id"]);
    advertised_urls(
        &app,
        &f.home,
        &admin.token,
        f.ticket.relay_url.as_deref().expect("route"),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/session_nodes/{node}/revoke"),
            Some(&admin.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let peers = session_nodes::peers(&f.home).expect("revoked state");
    assert!(
        peers
            .iter()
            .any(|peer| peer.node_id == node && !peer.enabled && !peer.connected)
    );
    // A hard relay removal can discard the QUIC close packet. Assert the
    // actual access boundary, as the existing transport fixture does.
    let denied =
        super::fixture::relay_client(&resumed, f.ticket.relay_url.as_deref().expect("relay"), key)
            .connect()
            .await;
    assert!(
        denied
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("Session-node relay authority"))
    );
    resumed.close().await;
    f.close().await;
}

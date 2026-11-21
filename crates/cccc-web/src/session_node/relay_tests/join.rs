//! Actual HTTP authorization, production issuance and an encrypted native Hello.
use super::fixture::Fixture;
use super::join_support::{advertised_urls, call};
use axum::http::StatusCode;
use cccc_core::{access_tokens::AccessTokenStore, session_nodes};
use serde_json::{Value, json};

const BASE: &str = "/api/v1/session_nodes/join";

#[tokio::test]
async fn http_join_requires_current_admin_then_enrolls_only_its_native_identity_and_revoke_wins() {
    let f = Fixture::new().await;
    let tokens = AccessTokenStore::new(f.home.clone()).expect("isolated token store");
    let admin = tokens
        .create("join-admin", vec![], true, Some("test-join-admin"))
        .expect("admin");
    let member = tokens
        .create("join-member", vec![], false, Some("test-join-member"))
        .expect("member");
    tokens
        .create("backup-admin", vec![], true, Some("test-backup-admin"))
        .expect("second admin");
    let app = crate::app(f.home.clone());
    let key = iroh::SecretKey::generate();
    let node = key.public().to_string();
    let input = |code: u8| json!({"node_id":node,"name":"Second laptop","device_code":format!("{code:064x}")});
    let (status, response) = call(&app, "POST", &format!("{BASE}/requests"), None, input(1)).await;
    assert_eq!(status, StatusCode::OK);
    let created = &response["result"];
    let id = created["request_id"].as_str().expect("public request ID");
    assert!(created.get("device_code").is_none() && created.get("ticket").is_none());
    let (_, retry) = call(&app, "POST", &format!("{BASE}/requests"), None, input(1)).await;
    assert_eq!(retry["result"]["request_id"], created["request_id"]);
    let poll = |id: &str, code: u8| json!({"request_id":id,"device_code":format!("{code:064x}")});
    let (_, pending) = call(&app, "POST", &format!("{BASE}/poll"), None, poll(id, 1)).await;
    assert_eq!(pending["result"]["status"], "pending");
    assert!(pending["result"].get("ticket").is_none());
    assert!(
        session_nodes::peers(&f.home)
            .expect("no enrollment")
            .is_empty()
    );
    assert_eq!(
        call(&app, "POST", &format!("{BASE}/poll"), None, poll(id, 2))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for path in [format!("{BASE}/pending"), format!("{BASE}/requests/{id}")] {
        assert_eq!(
            call(&app, "GET", &path, None, Value::Null).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, "GET", &path, Some(&member.token), Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    for action in ["approve", "deny"] {
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{BASE}/requests/{id}/{action}"),
                Some(&member.token),
                Value::Null
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let (_, detail) = call(
        &app,
        "GET",
        &format!("{BASE}/requests/{id}"),
        Some(&admin.token),
        Value::Null,
    )
    .await;
    assert_eq!(detail["result"]["request"]["status"], "pending");
    assert!(detail["result"]["request"].get("device_code").is_none());
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{BASE}/requests/{id}/approve"),
            Some(&admin.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        session_nodes::peers(&f.home)
            .expect("approval alone has no grant")
            .is_empty()
    );
    let (_, claim) = call(&app, "POST", &format!("{BASE}/poll"), None, poll(id, 1)).await;
    let encoded = claim["result"]["ticket"].as_str().expect("claimed ticket");
    let (_, repeated) = call(&app, "POST", &format!("{BASE}/poll"), None, poll(id, 1)).await;
    assert!(
        repeated["result"]["ticket"].as_str() == Some(encoded),
        "lost response keeps the same ticket"
    );
    let ticket = crate::session_node::decode_ticket(encoded).expect("native ticket");
    let endpoint = f.endpoint(key.clone(), Some(&ticket)).await;
    let (_, hello) = f.hello(&endpoint, Some(&ticket)).await;
    assert_eq!(hello["peer"]["node_id"], node);
    assert_eq!(hello["memberships"], json!([]));
    let (_, second) = call(&app, "POST", &format!("{BASE}/requests"), None, input(2)).await;
    let second_id = second["result"]["request_id"]
        .as_str()
        .expect("second request");
    session_nodes::revoke(&f.home, &node).expect("same persisted entry used by a separate CLI");
    for (request_id, code) in [(id, 1), (second_id, 2)] {
        let (_, revoked) = call(
            &app,
            "POST",
            &format!("{BASE}/poll"),
            None,
            poll(request_id, code),
        )
        .await;
        assert_eq!(revoked["result"]["status"], "denied");
        assert!(revoked["result"].get("ticket").is_none());
    }
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{BASE}/requests/{second_id}/approve"),
            Some(&admin.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, fresh) = call(&app, "POST", &format!("{BASE}/requests"), None, input(3)).await;
    let fresh_id = fresh["result"]["request_id"]
        .as_str()
        .expect("fresh request");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{BASE}/requests/{fresh_id}/approve"),
            Some(&admin.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, fresh_claim) = call(
        &app,
        "POST",
        &format!("{BASE}/poll"),
        None,
        poll(fresh_id, 3),
    )
    .await;
    let fresh_ticket = crate::session_node::decode_ticket(
        fresh_claim["result"]["ticket"]
            .as_str()
            .expect("new ticket"),
    )
    .expect("ticket");
    let fresh_endpoint = f.endpoint(key, Some(&fresh_ticket)).await;
    let (_, fresh_hello) = f.hello(&fresh_endpoint, Some(&fresh_ticket)).await;
    assert!(fresh_hello["peer"]["grant_id"] != hello["peer"]["grant_id"]);
    advertised_urls(
        &app,
        &f.home,
        &admin.token,
        f.ticket.relay_url.as_deref().expect("route"),
    )
    .await;
    tokens
        .delete(&admin.token_id())
        .expect("revoke administrator during page lifetime");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{BASE}/requests/{fresh_id}/approve"),
            Some(&admin.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let readonly = crate::app_with_mode(f.home.clone(), crate::WebMode::Exhibit);
    for path in [format!("{BASE}/requests"), format!("{BASE}/poll")] {
        assert_eq!(
            call(&readonly, "POST", &path, None, input(4)).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let disk = std::fs::read_to_string(f.home.root().join("state/session_nodes/state.json"))
        .expect("private state");
    assert!(
        !disk.contains(&format!("{:064x}", 1)),
        "no device credential on disk"
    );
    assert!(!disk.contains(encoded), "no issued ticket on disk");
    endpoint.close().await;
    fresh_endpoint.close().await;
    f.close().await;
}

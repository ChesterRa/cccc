use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn authenticated_delivery_is_idempotent_and_writes_remote_provenance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    integration_state::global_update(&home, "group_bridge", |value| {
        *value = json!({
            "registrations":[{
                "registration_id":"greg_test","group_id":group.group_id,
                "remote_group_id":"g_sender","remote_peer_id":"peer_sender",
                "credential":"secret-test","status":"active"
            }],
            "trusts":[{
                "trust_id":"trust_test","registration_id":"greg_test",
                "group_id":group.group_id,"status":"active","access_level":"messages"
            }],
            "deliveries":[]
        });
        Ok(())
    })
    .expect("bridge state");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    wait_for_daemon(&home).await;
    let app = cccc_web::app(home.clone());
    let payload = json!({
        "source_group_id":"g_sender","source_group_title":"Sender",
        "idempotency_key":"delivery-1","text":"hello remote","to":[]
    });

    let unauthorized = app
        .clone()
        .oneshot(request(&payload, None))
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);

    for expected_deduped in [false, true] {
        let response = app
            .clone()
            .oneshot(request(&payload, Some("secret-test")))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let result: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(result["result"]["deduped"], expected_deduped);
    }

    let messages: Vec<_> = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "chat.message")
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].by, "group_bridge:peer_sender");
    assert_eq!(messages[0].data["source_group_id"], "g_sender");
    assert_eq!(messages[0].data["source_platform"], "group_bridge_session");
    daemon.abort();
}

fn request(payload: &Value, credential: Option<&str>) -> Request<Body> {
    let mut builder = Request::post("/api/group-bridge/session/send")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(credential) = credential {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {credential}"));
    }
    builder
        .body(Body::from(payload.to_string()))
        .expect("request")
}

async fn wait_for_daemon(home: &HomeLayout) {
    let address = home.daemon_dir().join("ccccd.addr.json");
    for _ in 0..100 {
        if address.is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("daemon did not start");
}

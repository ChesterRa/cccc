//! Synthetic HTTP/WebSocket regression: no installed runtime, microphone or provider account.
use super::*;
use axum::{Json, Router, http::StatusCode, routing::post};
use cccc_contracts::codex_voice::{VoiceApplicationContext, VoiceCallMode};
use cccc_core::access_tokens::AccessTokenStore;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[tokio::test]
async fn persona_start_and_socket_ignore_analyst_and_notification_state() {
    let temp = tempfile::tempdir().expect("persona fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("persona fixture");
    home.initialize().expect("persona fixture");
    let instructions = "你是顾客。等待学员。".repeat(819) + "等待";
    assert_eq!(instructions.len(), 24_576);
    let context = VoiceApplicationContext::new_with_mode(
        "training".into(),
        instructions,
        VoiceCallMode::Persona,
    )
    .expect("persona fixture");
    let (requests, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let provider = Router::new().route(
        "/realtime/calls",
        post(move |Json(body): Json<Value>| {
            let requests = requests.clone();
            async move {
                requests.send(body).expect("persona fixture");
                (StatusCode::CREATED, "synthetic-answer-sdp")
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("persona fixture");
    let base_url = format!("http://{}", listener.local_addr().expect("persona fixture"));
    let provider = tokio::spawn(async move {
        axum::serve(listener, provider)
            .await
            .expect("persona fixture");
    });
    let auth_path = temp.path().join("auth.json");
    std::fs::write(
        &auth_path,
        r#"{"tokens":{"access_token":"fixture","account_id":"fixture"}}"#,
    )
    .expect("persona fixture");
    let config = RealtimeCallConfig {
        auth_path,
        base_url,
        voice: "cove".into(),
        preferences: Default::default(),
        application_context: Some(context.clone()),
    };
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let (router, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "persona-fixture".into(),
    );
    // A broken Analyst secret store and notification store must not prevent persona startup.
    let private_env = home.root().join("state/secrets/codex_voice_analyst.json");
    let notifications = home.root().join("state/codex_voice/notifications.json");
    for path in [&private_env, &notifications] {
        std::fs::create_dir_all(path.parent().expect("persona fixture")).expect("persona fixture");
        std::fs::write(path, "{").expect("persona fixture");
    }
    let StartOutcome::Started(started) = state
        .codex_voice
        .start_with_realtime(&home, "test", "offer", config.clone())
        .await
        .expect("persona fixture")
    else {
        panic!("new call");
    };
    let generation = started.session.info().generation;
    assert!(started.newly_created);
    assert_eq!(started.answer_sdp, "synthetic-answer-sdp");
    assert!(started.session.analyst().is_none());
    assert!(state.codex_voice.current().await.analyst.is_none());
    assert_eq!(started.session.info().mode, VoiceCallMode::Persona);
    assert_eq!(
        observed.recv().await.expect("persona fixture")["session"],
        json!({
            "model":"gpt-live-1-codex", "instructions":context.instructions(), "audio":{"output":{"voice":"cove"}},
        })
    );
    let StartOutcome::Started(repeated) = state
        .codex_voice
        .start_with_realtime(&home, "test", "offer", config.clone())
        .await
        .expect("persona fixture")
    else {
        panic!("idempotent call");
    };
    assert!(!repeated.newly_created);
    assert!(observed.try_recv().is_err());
    let assistant_context =
        VoiceApplicationContext::new(context.id().into(), context.instructions().into())
            .expect("persona fixture");
    let mut changed = config.clone();
    changed.application_context = Some(assistant_context);
    // Busy is decided without trying to load the unusable Analyst configuration.
    assert!(matches!(
        state
            .codex_voice
            .start_with_realtime(&home, "test", "offer", changed)
            .await
            .expect("persona fixture"),
        StartOutcome::Busy(_)
    ));
    for event in [
        json!({"type":"delegation.created"}),
        json!({"type":"delegation.created","item":{"text":"private"}}),
    ] {
        assert!(
            started
                .session
                .call()
                .route_provider_event(&generation, &event)
                .await
                .expect("persona fixture")
                .is_none()
        );
        assert!(
            started
                .session
                .call()
                .route_provider_event("stale", &event)
                .await
                .is_err()
        );
    }
    assert!(
        !started
            .session
            .call()
            .cancel_current(&generation)
            .await
            .expect("persona fixture")
    );

    // Queue a real notification/result, including a reservation for this generation.
    // Persona must not consume it or accept forged output receipts.
    use cccc_contracts::voice_notifications::NotificationScope;
    use cccc_core::voice_notifications as store;
    std::fs::remove_file(&notifications).expect("persona fixture");
    let groups = cccc_core::GroupStore::new(home.clone()).expect("persona fixture");
    let mut group = groups.create("Fixture", "").expect("persona fixture");
    group.actors.push(cccc_contracts::Actor::new("worker"));
    groups.save(&group).expect("persona fixture");
    let mut preferences = store::preferences(&home).expect("persona fixture");
    preferences
        .groups
        .insert(group.group_id.clone(), NotificationScope::AllChat);
    store::save_preferences(&home, preferences).expect("persona fixture");
    let mut event = cccc_contracts::Event::new("chat.message", &group.group_id);
    event.by = "worker".into();
    event.data = json!({"to":["user"],"text":"Synthetic result"})
        .as_object()
        .expect("persona fixture")
        .clone();
    cccc_core::ledger::append(
        &groups
            .ledger_path(&group.group_id)
            .expect("persona fixture"),
        &event,
    )
    .expect("persona fixture");
    store::scan(&home).expect("persona fixture");
    let source = store::snapshot(&home).expect("persona fixture").messages[0]
        .source
        .clone();
    store::reserve(&home, &source, "previous-analyst").expect("persona fixture");
    store::processed(
        &home,
        &[source.correlation_id()],
        "previous-analyst",
        "turn",
        "Synthetic summary",
    )
    .expect("persona fixture");
    let result_id = store::snapshot(&home).expect("persona fixture").results[0]
        .id
        .clone();
    store::reserve_output(&home, &result_id, &generation).expect("persona fixture");
    let notification_baseline = std::fs::read(&notifications).expect("persona fixture");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("persona fixture");
    let address = listener.local_addr().expect("persona fixture");
    let web = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("persona fixture");
    });
    let tokens = AccessTokenStore::new(home.clone()).expect("persona fixture");
    tokens
        .create("permanent owner", vec![], true, None)
        .expect("persona fixture");
    let owner = tokens
        .create("fixture", vec![], true, None)
        .expect("persona fixture");
    let client = reqwest::Client::new();
    let mut body = json!({"client_session_id":"test", "offer_sdp":"offer", "voice":"cove", "application_context":context});
    let response = client
        .post(format!("http://{address}/api/v1/codex_voice/calls"))
        .bearer_auth(&owner.token)
        .json(&body)
        .send()
        .await
        .expect("persona fixture");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("persona fixture");
    assert_eq!(response["result"]["call"]["mode"], "persona");
    assert!(response["result"]["analyst"].is_null());
    assert!(response["result"]["call"]["analyst_generation"].is_null());
    body["application_context"]["mode"] = json!("assistant");
    assert_eq!(
        client
            .post(format!("http://{address}/api/v1/codex_voice/calls"))
            .bearer_auth(&owner.token)
            .json(&body)
            .send()
            .await
            .expect("persona fixture")
            .status(),
        StatusCode::CONFLICT
    );
    body["application_context"]["mode"] = json!("unknown");
    assert_eq!(
        client
            .post(format!("http://{address}/api/v1/codex_voice/calls"))
            .bearer_auth(&owner.token)
            .json(&body)
            .send()
            .await
            .expect("persona fixture")
            .status(),
        StatusCode::BAD_REQUEST
    );
    for mode in ["assistant", "persona"] {
        body["application_context"]["mode"] = json!(mode);
        body["application_context"]["instructions"] = json!(format!("{}x", context.instructions()));
        let rejected = client
            .post(format!("http://{address}/api/v1/codex_voice/calls"))
            .bearer_auth(&owner.token)
            .json(&body)
            .send()
            .await
            .expect("persona fixture");
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let message = rejected.text().await.expect("persona fixture");
        assert!(message.contains("24576 UTF-8 bytes"));
        assert!(!message.contains("你是顾客"));
    }
    assert!(observed.try_recv().is_err());
    let mut request = format!("ws://{address}/api/v1/codex_voice/calls/{generation}/events")
        .into_client_request()
        .expect("persona fixture");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", owner.token)
            .parse()
            .expect("persona fixture"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("persona fixture");
    let ready = socket
        .next()
        .await
        .expect("persona fixture")
        .expect("persona fixture")
        .into_text()
        .expect("persona fixture");
    let ready: Value = serde_json::from_str(&ready).expect("persona fixture");
    assert_eq!(ready["call"]["mode"], "persona");
    assert!(ready["call"]["analyst_generation"].is_null());
    // The neutral opening is the only provider command. Ignored inputs don't generate speech/errors.
    let greeting = socket
        .next()
        .await
        .expect("persona fixture")
        .expect("persona fixture")
        .into_text()
        .expect("persona fixture");
    assert!(greeting.contains("whether to speak first or wait"));
    for event in [
        json!({"type":"provider_event","event":{"type":"delegation.created"}}),
        json!({"type":"notification_output_submitted","result_id":result_id}),
        json!({"type":"notification_output_not_submitted","result_ids":[result_id]}),
        json!({"type":"cancel_current"}),
        json!({"type":"heartbeat"}),
    ] {
        socket
            .send(Message::Text(event.to_string().into()))
            .await
            .expect("persona fixture");
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1300);
    while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, socket.next()).await {
        if let Message::Text(text) = message {
            assert_eq!(
                serde_json::from_str::<Value>(&text).expect("persona fixture")["type"],
                "heartbeat"
            );
        }
    }
    let denied = reqwest::Client::new()
        .post(format!(
            "http://{address}/api/v1/codex_voice/calls/{generation}/notification-output"
        ))
        .bearer_auth(&owner.token)
        .json(&json!({"result_id":result_id}))
        .send()
        .await
        .expect("persona fixture");
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        std::fs::read_to_string(&private_env).expect("persona fixture"),
        "{"
    );
    assert_eq!(
        std::fs::read(&notifications).expect("persona fixture"),
        notification_baseline
    );
    assert!(
        !home
            .root()
            .join("state/codex_voice/analyst-workdir")
            .exists()
    );
    assert!(
        state
            .codex_voice
            .current()
            .await
            .call
            .expect("persona fixture")
            .connected
    );
    tokens.delete(&owner.token_id()).expect("persona fixture");
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await
    .expect("idle persona must close after authorization revocation");
    // Lease release must allow another call, including after rejected provider startup.
    state
        .codex_voice
        .stop(&generation)
        .await
        .expect("persona fixture");
    let mut failed = config.clone();
    failed.auth_path = temp.path().join("missing-auth.json");
    assert!(
        state
            .codex_voice
            .start_with_realtime(&home, "fail", "offer", failed)
            .await
            .is_err()
    );
    assert!(state.codex_voice.current().await.call.is_none());
    let StartOutcome::Started(next) = state
        .codex_voice
        .start_with_realtime(&home, "next", "offer", config)
        .await
        .expect("persona fixture")
    else {
        panic!("lease released");
    };
    state
        .codex_voice
        .stop(&next.session.info().generation)
        .await
        .expect("persona fixture");
    let _ = shutdown.send(());
    web.abort();
    provider.abort();
}

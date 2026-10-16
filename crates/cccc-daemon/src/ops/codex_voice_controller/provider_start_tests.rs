use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn new_voice_uses_the_subscription_call_and_existing_client_delegation() {
    use axum::{
        Json, Router,
        extract::RawQuery,
        http::{HeaderMap, StatusCode},
        routing::post,
    };

    let voice = validate_realtime_voice(" Himari ").expect("new subscription voice");
    let temp = tempfile::tempdir().expect("fixture");
    let auth_path = temp.path().join("auth.json");
    std::fs::write(
        &auth_path,
        r#"{"tokens":{"access_token":"fixture-credential","account_id":"fixture-account"}}"#,
    )
    .expect("fixture auth");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener");
    let base_url = format!("http://{}", listener.local_addr().expect("fixture address"));
    let router = Router::new().route(
        "/realtime/calls",
        post(
            |RawQuery(query): RawQuery, headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(
                    query.as_deref(),
                    Some("intent=quicksilver&architecture=avas")
                );
                assert_eq!(headers["authorization"], "Bearer fixture-credential");
                assert_eq!(headers["chatgpt-account-id"], "fixture-account");
                assert_eq!(headers["openai-alpha"], "quicksilver=v2");
                assert_eq!(body["sdp"], "fixture-offer\r\n");
                assert_eq!(body["session"]["model"], "gpt-live-1-codex");
                assert_eq!(body["session"]["audio"]["output"]["voice"], "himari");
                assert_eq!(body["session"]["delegation"]["type"], "client");
                (StatusCode::CREATED, "fixture-answer\r\n")
            },
        ),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture server");
    });
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path,
        base_url,
        voice,
        preferences: Default::default(),
        application_context: None,
    };
    let answer = create_realtime_answer(&config, "fixture-offer\r\n").await;
    server.abort();
    assert_eq!(answer.expect("new voice call"), "fixture-answer\r\n");
}

#[tokio::test]
async fn rejected_start_preserves_http_status_without_upstream_body_or_credentials() {
    let temp = tempfile::tempdir().expect("fixture");
    let auth_path = temp.path().join("auth.json");
    std::fs::write(
        &auth_path,
        r#"{"tokens":{"access_token":"fixture-credential","account_id":"fixture-account"}}"#,
    )
    .expect("auth");
    for status in [401, 403, 429, 503] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request");
            let mut request = [0; 8192];
            let _ = socket.read(&mut request).await.expect("request bytes");
            let body = "private-upstream-body fixture-credential private-offer";
            let response = format!(
                "HTTP/1.1 {status} Rejected\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("response");
        });
        let config = RealtimeCallConfig {
            provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
            grok_executable: None,
            auth_path: auth_path.clone(),
            base_url,
            voice: DEFAULT_REALTIME_VOICE.into(),
            preferences: Default::default(),
            application_context: None,
        };
        let error = create_realtime_answer(&config, "private-offer")
            .await
            .expect_err("rejected");
        assert!(
            matches!(error.downcast_ref::<RealtimeCallError>(), Some(RealtimeCallError::HttpStatus(code)) if *code == status)
        );
        let chain = format!("{error:#}");
        for private in [
            "private-upstream-body",
            "fixture-credential",
            "private-offer",
        ] {
            assert!(!chain.contains(private));
        }
        server.await.expect("server");
    }
}

#[tokio::test]
async fn unreadable_invalid_and_incomplete_credentials_keep_the_auth_category() {
    let temp = tempfile::tempdir().expect("fixture");
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path: temp.path().join("auth.json"),
        base_url: "http://127.0.0.1:1".into(),
        voice: DEFAULT_REALTIME_VOICE.into(),
        preferences: Default::default(),
        application_context: None,
    };
    for content in [
        None,
        Some("not-json"),
        Some("{}"),
        Some(r#"{"tokens":{"access_token":"fixture"}}"#),
    ] {
        if let Some(content) = content {
            std::fs::write(&config.auth_path, content).expect("auth");
        }
        let error = create_realtime_answer(&config, "fixture-offer")
            .await
            .expect_err("auth failure");
        assert!(matches!(
            error.downcast_ref::<RealtimeCallError>(),
            Some(RealtimeCallError::Credentials)
        ));
    }
}

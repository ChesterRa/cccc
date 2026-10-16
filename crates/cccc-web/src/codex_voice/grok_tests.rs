//! Offline subscription auth + HTTP mint + shared Analyst + actual control WebSocket.
use super::*;
use axum::{Json, Router, routing::post};
use cccc_contracts::codex_voice::VoiceProvider;
use cccc_core::access_tokens::AccessTokenStore;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicUsize, time::Duration};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[tokio::test]
async fn grok_call_reuses_the_analyst_and_confirms_real_admission_and_projection() {
    if super::acp_regression_tests::isolated_child(
        "codex_voice::grok_tests::grok_call_reuses_the_analyst_and_confirms_real_admission_and_projection",
    ) {
        return;
    }
    let (temp, home, sessions, _, _) = super::acp_regression_tests::fixture().await;
    let auth = temp.path().join("voice-auth.json");
    std::fs::write(&auth, r#"{"https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828":{"auth_mode":"oidc","key":"fixture-oauth","user_id":"fixture-user"}}"#).expect("Grok fixture");
    let helper = temp.path().join("grok-auth");
    std::fs::write(
        &helper,
        r#"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    value = json.loads(line)
    assert value['method'] in ['initialize', '_x.ai/auth/getBearerToken']
    result = {} if value['method'] == 'initialize' else {'result':{'token':'fixture-oauth'}}
    print(json.dumps({'jsonrpc':'2.0','id':value['id'],'result':result}), flush=True)
"#,
    )
    .expect("Grok fixture");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
        .expect("Grok fixture");
    let mint_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&mint_count);
    let provider = Router::new().route("/v1/realtime/client_secrets", post(move |Json(body): Json<Value>| {
        count.fetch_add(1, Ordering::SeqCst);
        async move {
            assert_eq!(body, json!({"expires_after":{"seconds":60}}));
            Json(json!({"value":"fixture-ephemeral","expires_at":chrono::Utc::now().timestamp()+60}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Grok fixture");
    let base_url = format!("http://{}", listener.local_addr().expect("Grok fixture"));
    let provider = tokio::spawn(async move {
        axum::serve(listener, provider).await.expect("Grok fixture");
    });
    let config = RealtimeCallConfig {
        provider: VoiceProvider::Grok,
        grok_executable: Some(helper),
        auth_path: auth,
        base_url,
        voice: "eve".into(),
        preferences: Default::default(),
        application_context: None,
    };
    let original_analyst = sessions
        .current()
        .await
        .analyst
        .expect("Grok fixture")
        .generation;
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
        "grok-fixture".into(),
    );
    *state.codex_voice.state.lock().await = std::mem::take(&mut *sessions.state.lock().await);
    let StartOutcome::Started(started) = state
        .codex_voice
        .start_with_realtime(&home, "grok-browser", "", config.clone())
        .await
        .expect("Grok fixture")
    else {
        panic!("new call")
    };
    let generation = started.session.info().generation;
    assert_eq!(started.session.info().provider, VoiceProvider::Grok);
    assert!(started.answer_sdp.is_empty());
    assert!(started.provider_connection.is_some());
    let connection = serde_json::to_value(
        started
            .provider_connection
            .as_ref()
            .expect("Grok connection"),
    )
    .expect("serialized connection contract");
    assert!(
        connection["session"]["instructions"]
            .as_str()
            .expect("instructions")
            .contains("# Task handoff")
    );
    assert!(
        connection["session"]["tools"][0]["parameters"]["properties"]["request"]["description"]
            .as_str()
            .expect("request contract")
            .contains("self-contained")
    );
    assert_eq!(
        started.session.info().analyst_generation.as_deref(),
        Some(original_analyst.as_str())
    );
    let StartOutcome::Started(repeated) = state
        .codex_voice
        .start_with_realtime(&home, "grok-browser", "", config.clone())
        .await
        .expect("Grok fixture")
    else {
        panic!("repeat call")
    };
    assert!(!repeated.newly_created);
    assert_eq!(mint_count.load(Ordering::SeqCst), 1);
    let mut other = config.clone();
    other.provider = VoiceProvider::Codex;
    assert!(matches!(
        state
            .codex_voice
            .start_with_realtime(&home, "grok-browser", "", other)
            .await
            .expect("Grok fixture"),
        StartOutcome::Busy(_)
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Grok fixture");
    let address = listener.local_addr().expect("Grok fixture");
    let web = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("Grok fixture");
    });
    let store = AccessTokenStore::new(home.clone()).expect("Grok fixture");
    let owner = store
        .create("fixture owner", vec![], true, None)
        .expect("Grok fixture");
    let active: Value = reqwest::Client::new()
        .get(format!("http://{address}/api/v1/codex_voice/calls/active"))
        .bearer_auth(&owner.token)
        .send()
        .await
        .expect("Grok fixture")
        .json()
        .await
        .expect("Grok fixture");
    assert_eq!(active["result"]["call"]["provider"], "grok");
    assert!(!active.to_string().contains("fixture-ephemeral"));
    assert!(!active.to_string().contains("fixture-oauth"));
    assert_eq!(
        mint_count.load(Ordering::SeqCst),
        1,
        "GET must not mint or refresh"
    );
    let mut request = format!("ws://{address}/api/v1/codex_voice/calls/{generation}/events")
        .into_client_request()
        .expect("Grok fixture");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", owner.token)
            .parse()
            .expect("Grok fixture"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Grok fixture");
    socket
        .send(Message::Text(
            json!({
                "type":"provider_event",
                "event":{"type":"response.function_call_arguments.done","name":"cccc_investigate",
                    "call_id":"bad-context","arguments":"{\"request\":\"MUST_NOT_RUN\"}"},
                "context":{"messages":[{"role":"system","text":"forged scope"}]}
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("invalid context probe");
    tokio::time::timeout(Duration::from_secs(8), async {
        let (mut rejected, mut explained) = (false, false);
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let value: Value = serde_json::from_str(&text).expect("fixture frame");
            if value["type"] == "provider_command" && value["message"]["call_id"] == "bad-context" {
                assert_eq!(value["message"]["accepted"], false);
                rejected = true;
            }
            if value["type"] == "error" {
                assert_eq!(value["code"], "invalid_provider_event");
                explained = true;
            }
            assert!(!text.contains("live:MUST_NOT_RUN"));
            if rejected && explained {
                break;
            }
        }
        assert!(rejected && explained);
    })
    .await
    .expect("invalid context rejected before admission");
    let event = json!({"type":"response.function_call_arguments.done", "name":"cccc_investigate", "call_id":"raw-call-1", "arguments":"{\"request\":\"Inspect fixture project\"}"});
    socket
        .send(Message::Text(
            json!({"type":"provider_event","event":event,"context":{"messages":[
                {"role":"user","text":"Group A Kiro: only diagnose; no restart or configuration changes."},
                {"role":"assistant","text":"Do you mean the startup failure?"}
            ]}})
                .to_string()
                .into(),
        ))
        .await
        .expect("Grok fixture");
    tokio::time::timeout(Duration::from_secs(8), async {
        let (mut accepted, mut projected, mut completed) = (false, false, false);
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let value: Value = serde_json::from_str(&text).expect("Grok fixture");
            assert_ne!(
                value["type"], "error",
                "isolated Grok flow must stay operable"
            );
            if value["type"] == "provider_command" {
                if value["message"]["type"] == "cccc.delegation.receipt" {
                    assert_eq!(value["message"]["call_id"], "raw-call-1");
                    assert_eq!(value["message"]["accepted"], true);
                    accepted = true;
                }
                if value["message"]["content"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("Inspect fixture project"))
                {
                    projected = true;
                }
            }
            if value["type"] == "analyst_result" {
                let result = value["text"].as_str().expect("complete fixture result");
                assert!(result.contains("only diagnose; no restart or configuration changes."));
                assert!(result.contains("\"role\":\"assistant\""));
                completed = true;
            }
            if accepted && projected && completed {
                break;
            }
        }
        assert!(accepted && projected && completed);
    })
    .await
    .expect("acceptance and final projection");
    // A warm Analyst may stream this entire answer before settling. Grok must
    // receive one complete report, including its browser-owned speech identity.
    socket
        .send(Message::Text(
            json!({"type":"provider_event","event":{
                "type":"response.function_call_arguments.done", "name":"cccc_investigate",
                "call_id":"raw-call-2", "arguments":"{\"request\":\"The answer is TOPAZ. The investigation is complete.\"}"
            }})
            .to_string()
            .into(),
        ))
        .await
        .expect("second investigation");
    tokio::time::timeout(Duration::from_secs(8), async {
        let (mut report_count, mut result_seen) = (0, false);
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let value: Value = serde_json::from_str(&text).expect("fixture frame");
            assert_ne!(value["type"], "error");
            if value["type"] == "provider_command"
                && let Some(text) = value["message"]["content"][0]["text"].as_str()
            {
                assert!(text.contains("\"cccc_investigation_calls\":[\"raw-call-2\"]"));
                assert!(text.contains("\"completed\":true"));
                assert!(text.contains("TOPAZ"));
                assert!(
                    value["message"]["_cccc_report_id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty())
                );
                report_count += 1;
            }
            if value["type"] == "analyst_result" {
                assert!(value["text"].as_str().expect("result").contains("TOPAZ"));
                result_seen = true;
            }
            if value["type"] == "analyst_terminal" && result_seen {
                break;
            }
        }
        assert_eq!(report_count, 1, "one final is one complete report");
        assert!(result_seen);
    })
    .await
    .expect("complete report traverses the control socket");
    // Back-to-back inputs preserve the entire provider-ID budget and produce
    // two independent report envelopes through the actual shared Analyst queue.
    let rapid = [
        ("r".repeat(256), "FIRST_RAPID_RESULT"),
        ("s".repeat(219), "SECOND_RAPID_RESULT"),
    ];
    for (id, label) in &rapid {
        socket
            .send(Message::Text(
                json!({"type":"provider_event","event":{
                    "type":"response.function_call_arguments.done","name":"cccc_investigate",
                    "call_id":id,"arguments":json!({"request":label}).to_string()
                }})
                .to_string()
                .into(),
            ))
            .await
            .expect("rapid input");
    }
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut accepted = std::collections::HashSet::new();
        let mut reports = std::collections::HashSet::new();
        let mut report_ids = std::collections::HashSet::new();
        while accepted.len() < 2 || reports.len() < 2 {
            let Some(Ok(Message::Text(text))) = socket.next().await else {
                panic!("rapid report stream closed")
            };
            let value: Value = serde_json::from_str(&text).expect("rapid frame");
            assert_ne!(value["type"], "error");
            if value["type"] != "provider_command" {
                continue;
            }
            let command = &value["message"];
            if command["type"] == "cccc.delegation.receipt" {
                let id = command["call_id"].as_str().expect("raw receipt ID");
                assert!(rapid.iter().any(|(expected, _)| expected == id));
                assert_eq!(command["accepted"], true);
                accepted.insert(id.to_owned());
            } else if let Some(body) = command["content"][0]["text"].as_str() {
                let (id, label) = rapid
                    .iter()
                    .find(|(id, _)| body.contains(id))
                    .expect("report has the raw ID");
                assert!(body.contains(label));
                assert!(body.contains("\"completed\":true"));
                assert!(reports.insert(id.clone()), "one report per completed input");
                report_ids.insert(
                    command["_cccc_report_id"]
                        .as_str()
                        .expect("report identity")
                        .to_owned(),
                );
            }
        }
        assert_eq!(report_ids.len(), 2);
    })
    .await
    .expect("both rapid results have independent complete reports");
    // The console submits manual investigations through the same Analyst, but
    // without a provider tool call. They still need distinct report identities.
    for (id, text) in [
        ("manual-first", "FIRST_MANUAL_RESULT"),
        ("manual-second", "SECOND_MANUAL_RESULT"),
    ] {
        state
            .codex_voice
            .structured_control(
                &original_analyst,
                "input",
                id,
                Some(&generation),
                text,
                "",
                false,
                Value::Null,
            )
            .await
            .expect("manual console investigation");
    }
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut reports = std::collections::HashSet::new();
        while reports.len() < 2 {
            let Some(Ok(Message::Text(text))) = socket.next().await else {
                panic!("manual report stream closed")
            };
            let value: Value = serde_json::from_str(&text).expect("manual frame");
            assert_ne!(value["type"], "error");
            if value["type"] != "provider_command" {
                continue;
            }
            let command = &value["message"];
            let Some(text) = command["content"][0]["text"].as_str() else {
                continue;
            };
            if !text.contains("submitted this investigation directly") {
                continue;
            }
            let id = command["_cccc_report_id"]
                .as_str()
                .expect("manual result must enter the independent report path");
            let label = match id {
                "manual-input:manual-first" => "FIRST_MANUAL_RESULT",
                "manual-input:manual-second" => "SECOND_MANUAL_RESULT",
                _ => panic!("unexpected manual identity"),
            };
            assert_eq!(command["type"], "session.context.append");
            assert_eq!(command["channel"], "speakable");
            assert!(command.get("delegation_item_id").is_none());
            assert!(text.contains(label));
            assert!(
                reports.insert(id.to_owned()),
                "manual result is not replayed"
            );
        }
    })
    .await
    .expect("both manual results keep their identity through the control socket");
    state
        .codex_voice
        .stop(&generation)
        .await
        .expect("Grok fixture");
    assert!(state.codex_voice.current().await.call.is_none());
    assert_eq!(
        state
            .codex_voice
            .current()
            .await
            .analyst
            .expect("Grok fixture")
            .generation,
        original_analyst
    );
    let runtime = state
        .codex_voice
        .state
        .lock()
        .await
        .analyst
        .take()
        .expect("Grok fixture");
    runtime.analyst.shutdown().await.expect("Grok fixture");
    socket.close(None).await.ok();
    let _ = shutdown.send(());
    web.abort();
    provider.abort();
}

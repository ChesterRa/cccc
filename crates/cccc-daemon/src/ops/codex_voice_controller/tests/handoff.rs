use super::super::*;
use super::fake_server::recording_analyst_server;
use crate::ops::codex_voice_analyst::{AnalystSession, WorkspaceBinding};
use cccc_contracts::codex_voice::{VoiceApplicationContext, VoiceHandoffContext, VoiceProvider};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

fn event(provider: VoiceProvider, id: &str, request: &str) -> Value {
    match provider {
        VoiceProvider::Codex => json!({"type":"delegation.created","item":{
            "type":"delegation","target":"client","id":id,
            "content":[{"type":"input_text","text":request}]
        }}),
        VoiceProvider::Grok => json!({"type":"response.function_call_arguments.done",
            "name":"cccc_investigate","call_id":id,
            "arguments":json!({"request":request}).to_string()
        }),
    }
}

#[tokio::test]
async fn both_providers_deliver_antecedents_limits_and_corrections_to_the_same_turn() {
    for provider in [VoiceProvider::Codex, VoiceProvider::Grok] {
        let temp = tempfile::tempdir().expect("isolated fixture");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        home.initialize().expect("initialize");
        let (endpoint, server, starts, steers, inputs) = recording_analyst_server().await;
        let session = AnalystSession::connect_for_test(
            WorkspaceBinding {
                root: temp.path().into(),
            },
            "handoff-fixture".into(),
            endpoint,
            "codex".into(),
        )
        .await
        .expect("fake Analyst");
        let analyst = Arc::new(CodexVoiceAnalyst::from_session(session));
        let host = VoiceApplicationContext::new(
            "embedded".into(),
            "Follow this host's response policy.".into(),
        )
        .expect("host context");
        let call = CodexVoiceCall::start_with_provider(
            &home,
            Some(Arc::clone(&analyst)),
            Some(host.clone()),
            provider,
        )
        .await
        .expect("call");
        let context: VoiceHandoffContext = serde_json::from_value(json!({"messages":[
            {"role":"user","text":"A组 Kiro 启动失败，只查原因，不改配置，不重启。"},
            {"role":"assistant","text":"你想查启动失败的原因吗？"},
            {"role":"user","text":"对，就查这个。","incomplete":true}
        ]}))
        .expect("voice context");
        let first_event = event(provider, "handoff-first", "对，就查这个。");
        let first = call
            .route_provider_event(call.generation(), &first_event, &context)
            .await
            .expect("admit first");
        assert!(matches!(first, Some(VoiceDelegationAdmission::Turn(_))));
        call.route_provider_event(call.generation(), &first_event, &context)
            .await
            .expect("idempotent duplicate");
        call.route_provider_event(
            call.generation(),
            &event(
                provider,
                "handoff-correction",
                "改为B组的Kiro，仍然只查原因。",
            ),
            &context,
        )
        .await
        .expect("steer correction");
        assert!(
            call.route_provider_event("stale-call", &first_event, &context)
                .await
                .is_err()
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(steers.load(Ordering::SeqCst), 1);
        {
            let inputs = inputs.lock().expect("captured runtime inputs");
            assert_eq!(inputs.len(), 2);
            assert_eq!(
                inputs[0],
                host.analyst_input(&context.analyst_input("对，就查这个。"))
            );
            assert_eq!(
                inputs[1],
                host.analyst_input(&context.analyst_input("改为B组的Kiro，仍然只查原因。"))
            );
            assert!(inputs[0].contains("不改配置，不重启"));
            assert!(inputs[1].contains("改为B组"));
            assert!(inputs[0].contains("\"role\":\"assistant\""));
            assert!(inputs[0].contains("\"incomplete\":true"));
        }
        call.stop(call.generation()).await.expect("stop call");
        analyst.shutdown().await.expect("stop fake Analyst");
        server.await.expect("fixture assertions");
    }
}

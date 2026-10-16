use super::super::*;
use super::fake_server::{fake_analyst_server, fake_disconnecting_analyst_server};
use crate::ops::codex_voice_analyst::{AnalystSession, WorkspaceBinding};
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn grok_preserves_the_complete_raw_call_id_budget_after_namespacing() {
    use cccc_contracts::codex_voice::VoiceProvider;
    let temp = tempfile::tempdir().expect("Grok fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    let (endpoint, server, _, _) = fake_analyst_server().await;
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: temp.path().into(),
        },
        "call-id-budget".into(),
        endpoint,
        "codex".into(),
    )
    .await
    .expect("analyst");
    let analyst = Arc::new(CodexVoiceAnalyst::from_session(session));
    let mut lifecycle_events = analyst.subscribe_lifecycle();
    let mut pending_turns = std::collections::HashSet::new();
    let call = CodexVoiceCall::start_with_provider(
        &home,
        Some(Arc::clone(&analyst)),
        None,
        VoiceProvider::Grok,
    )
    .await
    .expect("call");
    for length in [218, 219, 256] {
        let raw = "r".repeat(length);
        let event = json!({"type":"response.function_call_arguments.done","name":"cccc_investigate","call_id":raw,"arguments":"{\"request\":\"Inspect this synthetic fixture\"}"});
        let admission = call
            .route_provider_event(call.generation(), &event, &Default::default())
            .await
            .expect("valid raw provider ID must be admitted")
            .expect("tool");
        let VoiceDelegationAdmission::Turn(receipt) = admission else {
            panic!("fixture admits a controlled turn")
        };
        assert_eq!(
            receipt.delegation_id,
            format!("grok:{}:{raw}", call.generation())
        );
        pending_turns.insert(receipt.turn_id);
    }
    let invalid = json!({"type":"response.function_call_arguments.done","name":"cccc_investigate","call_id":"r".repeat(257),"arguments":"{\"request\":\"Invalid ID must not launch work\"}"});
    assert!(
        call.route_provider_event(call.generation(), &invalid, &Default::default())
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while !pending_turns.is_empty() {
            if let AnalystLifecycleEvent::Completed { turn_id, .. } =
                lifecycle_events.recv().await.expect("lifecycle event")
            {
                pending_turns.remove(&turn_id);
            }
        }
    })
    .await
    .expect("fixture completes before cleanup");
    call.stop(call.generation()).await.expect("stop");
    analyst.shutdown().await.expect("cleanup");
    server.await.expect("fixture");
}

#[tokio::test]
async fn grok_reports_the_complete_final_once_while_codex_retains_native_progress() {
    use cccc_contracts::codex_voice::VoiceProvider;
    let temp = tempfile::tempdir().expect("projection fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    for provider in [VoiceProvider::Grok, VoiceProvider::Codex] {
        let lease = CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:projection")
            .expect("call lease");
        let call = CodexVoiceCall {
            provider,
            application_context: None,
            generation: "projection".into(),
            analyst: None,
            lease,
            state: tokio::sync::Mutex::new(CallState::default()),
        };
        let receipt = TurnReceipt {
            thread_id: "fixture-thread".into(),
            turn_id: "fixture-turn".into(),
            delegation_id: "grok:projection:request".into(),
        };
        call.follow_analyst_turn(&receipt).await;
        let final_text = "The investigation is finished. The result is TOPAZ.";
        let progress = call
            .project_analyst_delta("projection", &receipt.turn_id, final_text)
            .await
            .expect("progress");
        if provider == VoiceProvider::Grok {
            assert!(
                progress.is_empty(),
                "Grok reports complete results, not partial fragments"
            );
        } else {
            assert!(!progress.is_empty());
        }
        assert!(
            call.take_final_projection(
                "stale",
                &receipt.delegation_id,
                &receipt.turn_id,
                final_text
            )
            .await
            .is_err()
        );
        let commands = call
            .take_final_projection(
                "projection",
                &receipt.delegation_id,
                &receipt.turn_id,
                final_text,
            )
            .await
            .expect("final")
            .expect("first completion")
            .commands;
        if provider == VoiceProvider::Grok {
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0]["channel"], "speakable");
            let text = commands[0]["content"][0]["text"]
                .as_str()
                .expect("completion text");
            assert!(text.contains("\"cccc_investigation_calls\":[\"request\"]"));
            assert!(text.contains("\"completed\":true"));
            assert!(
                text.contains(final_text),
                "the report has its entire authoritative final"
            );
            assert_eq!(commands[0]["_cccc_report_id"], receipt.turn_id);
        } else {
            assert!(
                commands.is_empty(),
                "preserve the native Codex completion protocol"
            );
        }
        assert!(
            call.take_final_projection(
                "projection",
                &receipt.delegation_id,
                &receipt.turn_id,
                final_text,
            )
            .await
            .expect("repeat")
            .is_none()
        );
        assert!(
            call.project_analyst_delta("projection", &receipt.turn_id, "late progress")
                .await
                .expect("late progress")
                .is_empty()
        );
        call.stop("projection").await.expect("release lease");
    }
}

#[tokio::test]
async fn grok_request_ids_are_fenced_by_call_while_projected_context_retains_raw_ids() {
    use cccc_contracts::codex_voice::VoiceProvider;
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().expect("Grok fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("home");
    let (endpoint, server, starts, steers) = fake_analyst_server().await;
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: temp.path().into(),
        },
        "shared-analyst".into(),
        endpoint,
        "codex".into(),
    )
    .await
    .expect("Analyst fixture");
    let analyst = Arc::new(CodexVoiceAnalyst::from_session(session));
    let event = json!({"type":"response.function_call_arguments.done", "name":"cccc_investigate", "call_id":"same-raw-call", "arguments":"{\"request\":\"Inspect the current request\"}"});
    let mut identities = Vec::new();
    for _ in 0..2 {
        let call = CodexVoiceCall::start_with_provider(
            &home,
            Some(Arc::clone(&analyst)),
            None,
            VoiceProvider::Grok,
        )
        .await
        .expect("Grok call");
        let VoiceDelegationAdmission::Turn(receipt) = call
            .route_provider_event(call.generation(), &event, &Default::default())
            .await
            .expect("admitted")
            .expect("tool")
        else {
            panic!("controlled fixture turn");
        };
        identities.push(receipt.delegation_id.clone());
        assert!(
            receipt
                .delegation_id
                .starts_with(&format!("grok:{}:", call.generation()))
        );
        assert!(
            call.route_provider_event("stale-call", &event, &Default::default())
                .await
                .is_err()
        );
        call.route_provider_event(call.generation(), &event, &Default::default())
            .await
            .expect("duplicate is idempotent");
        let commands = call
            .take_final_projection(
                call.generation(),
                &receipt.delegation_id,
                &receipt.turn_id,
                "Scoped final result",
            )
            .await
            .expect("project")
            .expect("result")
            .commands;
        let text = commands
            .iter()
            .filter_map(|command| command["content"][0]["text"].as_str())
            .collect::<String>();
        assert!(text.contains("\"cccc_investigation_calls\":[\"same-raw-call\"]"));
        assert!(text.contains("\"completed\":true"));
        assert!(text.contains("Scoped final result"));
        call.stop(call.generation()).await.expect("stop audio");
    }
    assert_ne!(identities[0], identities[1]);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(steers.load(Ordering::SeqCst), 1);
    analyst.shutdown().await.expect("cleanup");
    server.await.expect("fixture");
}

#[tokio::test]
async fn application_context_reaches_new_and_steered_delegations_without_replay() {
    use cccc_contracts::codex_voice::VoiceApplicationContext;
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("isolated home");
    home.initialize().expect("initialize home");
    let context = VoiceApplicationContext::new(
        "work:current".into(),
        "日本語。現在の仕事の権限をツールで確認する。".into(),
    )
    .expect("valid context");
    let (endpoint, server, starts, steers) =
        super::fake_server::context_analyst_server(context.analyst_input("")).await;
    let analyst = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: temp.path().to_path_buf(),
        },
        "context-analyst".into(),
        endpoint,
        "codex".into(),
    )
    .await
    .expect("connect fixture Analyst");
    let analyst = Arc::new(CodexVoiceAnalyst::from_session(analyst));
    let call = CodexVoiceCall::start(&home, Some(Arc::clone(&analyst)), Some(context))
        .await
        .expect("start embedded call");
    for id in ["first", "first", "second"] {
        call.route_provider_event(
            call.generation(),
            &json!({
                "type":"delegation.created", "item":{
                    "type":"delegation", "target":"client", "id":id,
                    "content":[{"type":"input_text","text":"現在の材料単価を比較する"}]
                }
            }),
            &Default::default(),
        )
        .await
        .expect("admit delegation")
        .expect("delegation");
    }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(steers.load(Ordering::SeqCst), 1);
    call.stop(call.generation()).await.expect("stop call");
    analyst.shutdown().await.expect("stop fixture Analyst");
    server.await.expect("fixture server assertions");
}

#[tokio::test]
async fn stopping_audio_keeps_the_shared_analyst_available_for_the_next_call() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("root");
    let (endpoint, server, _, _) = fake_analyst_server().await;
    let analyst = AnalystSession::connect_for_test(
        WorkspaceBinding { root },
        "analyst-shared".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("Analyst");
    let analyst = Arc::new(CodexVoiceAnalyst::from_session(analyst));
    let first_lease = CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-r1")
        .expect("first call lease");
    let first = CodexVoiceCall {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        application_context: None,
        generation: "call-r1".into(),
        analyst: Some(Arc::clone(&analyst)),
        lease: first_lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    assert_eq!(first.analyst_thread_id(), "thread-controller");
    first.stop("call-r1").await.expect("stop first audio call");

    let context = cccc_contracts::codex_voice::VoiceApplicationContext::new_with_mode(
        "training".into(),
        "Act as a customer.".into(),
        cccc_contracts::codex_voice::VoiceCallMode::Persona,
    )
    .expect("persona context");
    assert!(CodexVoiceCall::start(&home, None, None).await.is_err());
    assert!(
        CodexVoiceCall::start(&home, Some(Arc::clone(&analyst)), Some(context.clone()))
            .await
            .is_err()
    );
    let persona = CodexVoiceCall::start(&home, None, Some(context))
        .await
        .expect("persona between assistant calls");
    assert!(persona.analyst().is_none());
    persona
        .stop(persona.generation())
        .await
        .expect("stop persona");

    let second_lease = CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-r2")
        .expect("second call lease");
    let second = CodexVoiceCall {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        application_context: None,
        generation: "call-r2".into(),
        analyst: Some(Arc::clone(&analyst)),
        lease: second_lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    assert_eq!(second.analyst_thread_id(), "thread-controller");
    assert_eq!(
        second.analyst().expect("assistant fixture").generation(),
        "analyst-shared"
    );
    second
        .stop("call-r2")
        .await
        .expect("stop second audio call");
    assert_eq!(
        voice_recording_lease::current(&home).expect("recording lease state"),
        json!({})
    );
    analyst.shutdown().await.expect("stop shared Analyst");
    server.await.expect("fake Analyst server");
}

#[tokio::test]
async fn analyst_disconnect_is_generation_bound_and_call_drop_releases_the_lease() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("root");
    let (endpoint, server) = fake_disconnecting_analyst_server().await;
    let analyst = AnalystSession::connect_for_test(
        WorkspaceBinding { root },
        "analyst-disconnect".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("Analyst");
    let lease =
        CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-d").expect("call lease");
    let call = CodexVoiceCall {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        application_context: None,
        generation: "call-d".into(),
        analyst: Some(Arc::new(CodexVoiceAnalyst::from_session(analyst))),
        lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    let mut events = call.subscribe_analyst();
    assert!(
        call.begin_delegation(
            "call-d",
            &ProviderDelegation {
                id: "provider-ambiguous".into(),
                text: "disconnect while starting".into(),
            },
        )
        .await
        .is_err()
    );
    let disconnected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("Analyst event");
            if event.message["method"]
                == super::super::super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD
            {
                return event;
            }
        }
    })
    .await
    .expect("disconnect event");
    assert_eq!(disconnected.generation, "analyst-disconnect");
    drop(call);
    assert_eq!(
        voice_recording_lease::current(&home).expect("recording lease state"),
        json!({})
    );
    server.await.expect("disconnecting Analyst server");
}

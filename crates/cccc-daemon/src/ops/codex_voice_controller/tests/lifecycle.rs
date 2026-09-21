use super::super::*;
use super::fake_server::{fake_analyst_server, fake_disconnecting_analyst_server};
use crate::ops::codex_voice_analyst::{AnalystSession, WorkspaceBinding};
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
    let call = CodexVoiceCall::start(&home, Arc::clone(&analyst), Some(context))
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
        application_context: None,
        generation: "call-r1".into(),
        analyst: Arc::clone(&analyst),
        lease: first_lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    assert_eq!(first.analyst_thread_id(), "thread-controller");
    first.stop("call-r1").await.expect("stop first audio call");

    let second_lease = CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-r2")
        .expect("second call lease");
    let second = CodexVoiceCall {
        application_context: None,
        generation: "call-r2".into(),
        analyst: Arc::clone(&analyst),
        lease: second_lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    assert_eq!(second.analyst_thread_id(), "thread-controller");
    assert_eq!(second.analyst.generation(), "analyst-shared");
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
        application_context: None,
        generation: "call-d".into(),
        analyst: Arc::new(CodexVoiceAnalyst::from_session(analyst)),
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

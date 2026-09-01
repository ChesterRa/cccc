use super::super::*;
use super::fake_server::{fake_analyst_server, fake_disconnecting_analyst_server};
use crate::ops::codex_voice_analyst::{AnalystSession, ScopeBinding};
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn stopping_audio_keeps_the_shared_analyst_available_for_the_next_call() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("root");
    let (endpoint, server, _, _) = fake_analyst_server().await;
    let analyst = AnalystSession::connect_for_test(
        ScopeBinding {
            group_id: "g_voice".into(),
            root,
        },
        "analyst-shared".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
    )
    .await
    .expect("Analyst");
    let analyst = Arc::new(CodexVoiceAnalyst::from_session(analyst));
    let first_lease = CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-r1")
        .expect("first call lease");
    let first = CodexVoiceCall {
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
        ScopeBinding {
            group_id: "g_voice".into(),
            root,
        },
        "analyst-disconnect".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
    )
    .await
    .expect("Analyst");
    let lease =
        CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-d").expect("call lease");
    let call = CodexVoiceCall {
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
            if event.message["method"] == "cccc/voiceAnalyst/disconnected" {
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

use super::super::*;
use super::fake_server::gated_analyst_server;
use crate::ops::codex_voice_analyst::{AnalystSession, WorkspaceBinding};
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
#[tokio::test]
async fn call_generation_coalesces_delegations_and_projects_progress_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("root");
    let (endpoint, server, starts, steers, release_interrupt) = gated_analyst_server().await;
    let analyst = AnalystSession::connect_for_test(
        WorkspaceBinding { root: root.clone() },
        "analyst-generation".into(),
        endpoint.clone(),
        PathBuf::from("codex"),
    )
    .await
    .expect("Analyst");
    let lease =
        CallLease::acquire(&home, "g_voice", "Voice", "codex-voice:call-a").expect("call lease");
    let call = CodexVoiceCall {
        generation: "call-a".into(),
        analyst: Arc::new(CodexVoiceAnalyst::from_session(analyst)),
        lease,
        state: tokio::sync::Mutex::new(CallState::default()),
    };
    assert_eq!(call.generation(), "call-a");
    assert_eq!(call.analyst_thread_id(), "thread-controller");
    assert_eq!(call.analyst_tui_command()[2], endpoint);
    call.heartbeat("call-a").expect("heartbeat");
    let mut lifecycle_events = call.analyst.subscribe_lifecycle();
    let event = json!({
        "type":"delegation.created",
        "item":{
            "type":"delegation", "target":"client", "id":"provider-delegation-1",
            "content":[{"type":"input_text","text":"inspect the repository"}]
        }
    });
    let first = call
        .begin_provider_event("call-a", &event)
        .await
        .expect("begin")
        .expect("delegation");
    let replay = call
        .begin_provider_event("call-a", &event)
        .await
        .expect("replay")
        .expect("delegation");
    assert_eq!(first, replay);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(call.begin_provider_event("stale", &event).await.is_err());
    let second = call
        .begin_delegation(
            "call-a",
            &ProviderDelegation {
                id: "provider-delegation-2".into(),
                text: "also inspect the single word inside".into(),
            },
        )
        .await
        .expect("steer second delegation into active turn");
    assert_eq!(second.turn_id, first.turn_id);
    assert_eq!(second.thread_id, first.thread_id);
    assert_eq!(second.delegation_id, "provider-delegation-2");
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(steers.load(Ordering::SeqCst), 1);
    call.steer("call-a", "provider-delegation-2", "add tests")
        .await
        .expect("explicit steer");
    assert_eq!(steers.load(Ordering::SeqCst), 2);

    let progress = call
        .project_analyst_delta(
            "call-a",
            &first.turn_id,
            "I found the requested file. I am checking its exact content.",
        )
        .await
        .expect("buffered progress");
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0]["type"], "session.context.append");
    assert_eq!(
        progress[0]["content"][0]["text"],
        "I found the requested file. I am checking its exact content."
    );
    assert!(
        call.project_analyst_delta("call-a", &first.turn_id, " The answer is TOPAZ.")
            .await
            .expect("remaining progress")
            .is_empty()
    );
    let result =
        "I found the requested file. I am checking its exact content. The answer is TOPAZ.";
    let projection = call
        .take_final_projection("call-a", "provider-delegation-1", &first.turn_id, result)
        .await
        .expect("projection")
        .expect("first projection");
    assert_eq!(projection.delegation_id, "provider-delegation-2");
    assert_eq!(projection.commands.len(), 1);
    assert!(projection.commands.iter().all(|command| {
        command["type"] == "session.context.append" && command["channel"] == "speakable"
    }));
    assert_eq!(
        projection
            .commands
            .iter()
            .filter_map(|command| command["content"][0]["text"].as_str())
            .collect::<String>(),
        "The answer is TOPAZ."
    );
    assert!(
        call.take_final_projection(
            "call-a",
            "provider-delegation-2",
            &first.turn_id,
            "duplicate",
        )
        .await
        .expect("duplicate projection")
        .is_none()
    );
    let first_started_events = tokio::time::timeout(Duration::from_secs(2), async {
        let mut started_events = 0;
        loop {
            match lifecycle_events.recv().await.expect("lifecycle event") {
                AnalystLifecycleEvent::Started { .. } => started_events += 1,
                AnalystLifecycleEvent::Completed { .. } => break started_events,
                _ => {}
            }
        }
    })
    .await
    .expect("Analyst terminal boundary");
    assert_eq!(first_started_events, 1, "one turn has one lifecycle start");

    let third = call
        .begin_delegation(
            "call-a",
            &ProviderDelegation {
                id: "provider-delegation-3".into(),
                text: "start a new investigation".into(),
            },
        )
        .await
        .expect("new turn after final");
    assert_ne!(third.turn_id, first.turn_id);
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    call.cancel("call-a", "provider-delegation-3")
        .await
        .expect("cancel active investigation");
    assert!(
        call.begin_delegation(
            "call-a",
            &ProviderDelegation {
                id: "provider-while-cancelling".into(),
                text: "must wait for the terminal boundary".into(),
            },
        )
        .await
        .is_err()
    );
    assert!(
        call.settle_without_projection("call-a", &third.turn_id)
            .await
            .expect("settle interrupted turn")
    );
    assert!(
        !call
            .settle_without_projection("call-a", &third.turn_id)
            .await
            .expect("terminal replay is idempotent")
    );
    release_interrupt.send(()).expect("release terminal");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                lifecycle_events.recv().await.expect("lifecycle event"),
                AnalystLifecycleEvent::Completed { .. }
            ) {
                break;
            }
        }
    })
    .await
    .expect("cancelled Analyst terminal boundary");

    let fourth = call
        .begin_delegation(
            "call-a",
            &ProviderDelegation {
                id: "provider-delegation-4".into(),
                text: "start after cancellation settles".into(),
            },
        )
        .await
        .expect("new turn after cancellation");
    assert_ne!(fourth.turn_id, third.turn_id);
    assert_eq!(starts.load(Ordering::SeqCst), 3);
    let long_result = format!("{}界{}", "a".repeat(499), "b".repeat(501));
    let final_only = call
        .take_final_projection(
            "call-a",
            "provider-delegation-4",
            &fourth.turn_id,
            &long_result,
        )
        .await
        .expect("final-only projection")
        .expect("new final");
    assert!(final_only.commands.len() >= 3);
    assert!(final_only.commands.iter().all(|command| {
        command["type"] == "delegation.context.append"
            && command["delegation_item_id"] == "provider-delegation-4"
            && command["channel"] == "speakable"
    }));
    assert_eq!(
        final_only
            .commands
            .iter()
            .filter_map(|command| command["content"][0]["text"].as_str())
            .collect::<String>(),
        long_result
    );
    call.stop("call-a").await.expect("stop call");
    assert_eq!(
        voice_recording_lease::current(&home).expect("recording lease state"),
        json!({})
    );
    call.analyst.shutdown().await.expect("stop Analyst");
    server.await.expect("fake Analyst server");
}

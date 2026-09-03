#[path = "assistant_voice_ws_revision/support.rs"]
mod support;

use ::tokio_tungstenite::tungstenite::Message;
use cccc_core::{GroupStore, assistant_state, ledger};
use futures_util::SinkExt;
use support::*;

#[tokio::test]
async fn short_document_stop_persists_and_delivers_the_only_final_transcript() {
    let harness = setup().await;
    let lease_id = acquire_document_lease(&harness).await;
    let mut socket = start_document_recording(&harness, &lease_id, "session-short").await;
    send_audio_and_stop(&mut socket).await;
    let frames = collect_frames(&mut socket).await;
    assert!(frames.iter().any(|frame| {
        frame["type"] == "final_asr_text"
            && frame["text"] == "最终文本。"
            && frame["transcript_persisted"] == true
    }));

    let state = assistant_state::load(&harness.home, &harness.group_id).expect("assistant state");
    let session = state["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|item| item["session_id"] == "session-short")
        })
        .expect("persisted session");
    assert_eq!(session["transcript"], "最终文本。");
    assert_eq!(session["segments"].as_array().map(Vec::len), Some(1));
    assert_eq!(session["segments"][0]["transcript_stage"], "final");
    assert_eq!(
        voice_input_event_count(&harness),
        1,
        "a short recording must still create one semantic input"
    );
    shutdown_all(harness).await;
}

#[tokio::test]
async fn early_disconnect_persists_and_delivers_the_only_final_transcript() {
    let harness = setup().await;
    let lease_id = acquire_document_lease(&harness).await;
    let mut socket = start_document_recording(&harness, &lease_id, "session-disconnect").await;
    socket
        .send(Message::Binary(vec![0_u8; 1_600].into()))
        .await
        .expect("send audio");
    socket.close(None).await.expect("close websocket");

    let session = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let state =
                assistant_state::load(&harness.home, &harness.group_id).expect("assistant state");
            let session = state["sessions"].as_array().and_then(|sessions| {
                sessions
                    .iter()
                    .find(|item| item["session_id"] == "session-disconnect")
                    .cloned()
            });
            if let Some(session) = session.filter(|_| voice_input_event_count(&harness) == 1) {
                break session;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("disconnect finalization timeout");
    assert_eq!(session["transcript"], "最终文本。");
    assert_eq!(voice_input_event_count(&harness), 1);
    shutdown_all(harness).await;
}

#[tokio::test]
async fn final_persistence_failure_is_reported_to_the_websocket_caller() {
    let mut harness = setup().await;
    let lease_id = acquire_document_lease(&harness).await;
    let mut socket = start_document_recording(&harness, &lease_id, "session-failure").await;
    socket
        .send(Message::Binary(vec![0_u8; 1_600].into()))
        .await
        .expect("send audio");
    shutdown(&harness.home).await;
    (&mut harness.daemon)
        .await
        .expect("daemon task")
        .expect("daemon shutdown");

    send_stop(&mut socket).await;
    let frames = collect_frames(&mut socket).await;
    let final_event = frames
        .iter()
        .find(|frame| frame["type"] == "final_asr_text")
        .expect("final ASR event");
    assert_eq!(final_event["ok"], true);
    assert_eq!(final_event["transcript_persisted"], false);
    assert_eq!(final_event["transcript_persistence"], "failed");
    assert_eq!(
        final_event["transcript_persistence_error"]["code"],
        "daemon_unavailable"
    );
    harness.web.kill().await.expect("stop web child");
    let _ = harness.web.wait().await;
}

async fn send_audio_and_stop(socket: &mut VoiceSocket) {
    socket
        .send(Message::Binary(vec![0_u8; 1_600].into()))
        .await
        .expect("send audio");
    send_stop(socket).await;
}

async fn shutdown_all(mut harness: Harness) {
    harness.web.kill().await.expect("stop web child");
    let _ = harness.web.wait().await;
    shutdown(&harness.home).await;
    harness
        .daemon
        .await
        .expect("daemon task")
        .expect("daemon shutdown");
}

fn voice_input_event_count(harness: &Harness) -> usize {
    let events = ledger::read_all(
        &GroupStore::new(harness.home.clone())
            .expect("group store")
            .ledger_path(&harness.group_id)
            .expect("ledger path"),
    )
    .expect("ledger");
    events
        .iter()
        .filter(|event| event.kind == "assistant.voice.input")
        .count()
}

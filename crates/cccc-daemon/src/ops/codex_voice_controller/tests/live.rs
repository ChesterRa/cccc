use super::super::*;
use super::live_helper::{LiveVoiceHelper, required_env_path, send_helper_command};
use super::live_loop::run_integrated_loop;
use base64::Engine as _;
use cccc_core::{GroupStore, HomeLayout, group_scope, scope, voice_recording_lease};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[tokio::test]
async fn live_realtime_delegation_runs_through_the_group_bound_call_when_enabled() {
    if std::env::var("CCCC_CODEX_VOICE_INTEGRATED_LIVE").as_deref() != Ok("1") {
        return;
    }
    let helper_path = required_env_path("CCCC_CODEX_VOICE_HELPER");
    let pcm_path = required_env_path("CCCC_CODEX_VOICE_PCM");
    let auth_path = required_env_path("CCCC_CODEX_AUTH_PATH");
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("banana"), "TOPAZ\n").expect("fixture");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("integrated voice", "").expect("group");
    let group = group_scope::attach(
        &store,
        &group.group_id,
        scope::detect(&root).expect("scope"),
    )
    .expect("attach");
    let mut analyst_config = LaunchConfig::new(&group.group_id, &root);
    analyst_config.model =
        Some(std::env::var("CCCC_VOICE_ANALYST_MODEL").unwrap_or_else(|_| "gpt-5.6-sol".into()));
    analyst_config.codex_executable = std::env::var_os("CCCC_CODEX_EXECUTABLE").map(PathBuf::from);
    let call = CodexVoiceCall::launch(&home, analyst_config)
        .await
        .expect("launch integrated Codex Voice call");
    let generation = call.generation().to_owned();
    assert_eq!(
        voice_recording_lease::current(&home).expect("active lease")["capture_mode"],
        "codex_voice"
    );

    let mut helper = LiveVoiceHelper::start(&helper_path)
        .await
        .expect("voice helper");
    helper
        .send(json!({"type":"start_v3_bridge"}))
        .await
        .expect("start bridge");
    let offer = helper
        .wait_for(|event| event["type"] == "offer")
        .await
        .expect("offer")["sdp"]
        .as_str()
        .expect("offer SDP")
        .to_owned();
    let answer = create_realtime_answer(
        &RealtimeCallConfig {
            auth_path,
            base_url: std::env::var("CCCC_CODEX_VOICE_BASE_URL")
                .unwrap_or_else(|_| "https://chatgpt.com/backend-api/codex".into()),
            voice: "cove".into(),
        },
        &offer,
    )
    .await
    .expect("provider call setup");
    helper
        .send(json!({"type":"apply_answer","sdp":answer}))
        .await
        .expect("apply answer");
    helper
        .wait_for(|event| event["type"] == "state" && event["state"] == "ready")
        .await
        .expect("data channel ready");

    let input = helper.input();
    let stop_feed = Arc::new(AtomicBool::new(false));
    let feed_stop = Arc::clone(&stop_feed);
    let pcm = tokio::fs::read(&pcm_path).await.expect("PCM fixture");
    let feed = tokio::spawn(async move {
        for frame in pcm.chunks(960) {
            if feed_stop.load(Ordering::Acquire) {
                break;
            }
            send_helper_command(
                &input,
                &json!({
                    "type":"send_pcm",
                    "audio":base64::engine::general_purpose::STANDARD.encode(frame),
                    "sample_rate":24_000,
                    "num_channels":1,
                }),
            )
            .await
            .expect("feed PCM");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let report = run_integrated_loop(&call, &generation, &mut helper).await;
    stop_feed.store(true, Ordering::Release);
    feed.await.expect("PCM feed");
    helper.shutdown().await.expect("shutdown helper");
    call.stop(&generation).await.expect("stop integrated call");
    assert_eq!(
        voice_recording_lease::current(&home).expect("recording lease state"),
        json!({})
    );
    assert!(
        !report.delegated.is_empty(),
        "provider emitted no delegation"
    );
    assert_eq!(
        report.analyst_turn_ids.len(),
        1,
        "provider delegations must coalesce into one Analyst turn: {:?}",
        report.provider_delegations
    );
    assert_eq!(report.analyst_result, "TOPAZ");
    assert!(report.projection_sent);
    assert!(report.post_projection_audio_frames > 0);
    eprintln!(
        "Codex Voice result acknowledgements: {:?}",
        report.provider_ack_events
    );
}

#[tokio::test]
async fn live_realtime_identity_question_stays_in_voice_when_enabled() {
    if std::env::var("CCCC_CODEX_VOICE_ROUTING_LIVE").as_deref() != Ok("1") {
        return;
    }
    let helper_path = required_env_path("CCCC_CODEX_VOICE_HELPER");
    let pcm_path = required_env_path("CCCC_CODEX_VOICE_DIRECT_PCM");
    let auth_path = required_env_path("CCCC_CODEX_AUTH_PATH");
    let mut helper = LiveVoiceHelper::start(&helper_path)
        .await
        .expect("voice helper");
    helper
        .send(json!({"type":"start_v3_bridge"}))
        .await
        .expect("start bridge");
    let offer = helper
        .wait_for(|event| event["type"] == "offer")
        .await
        .expect("offer")["sdp"]
        .as_str()
        .expect("offer SDP")
        .to_owned();
    let answer = create_realtime_answer(
        &RealtimeCallConfig {
            auth_path,
            base_url: std::env::var("CCCC_CODEX_VOICE_BASE_URL")
                .unwrap_or_else(|_| "https://chatgpt.com/backend-api/codex".into()),
            voice: "cove".into(),
        },
        &offer,
    )
    .await
    .expect("provider call setup");
    helper
        .send(json!({"type":"apply_answer","sdp":answer}))
        .await
        .expect("apply answer");
    helper
        .wait_for(|event| event["type"] == "state" && event["state"] == "ready")
        .await
        .expect("data channel ready");

    let input = helper.input();
    let stop_feed = Arc::new(AtomicBool::new(false));
    let feed_stop = Arc::clone(&stop_feed);
    let pcm = tokio::fs::read(&pcm_path)
        .await
        .expect("direct PCM fixture");
    let feed = tokio::spawn(async move {
        for frame in pcm.chunks(960) {
            if feed_stop.load(Ordering::Acquire) {
                break;
            }
            send_helper_command(
                &input,
                &json!({
                    "type":"send_pcm",
                    "audio":base64::engine::general_purpose::STANDARD.encode(frame),
                    "sample_rate":24_000,
                    "num_channels":1,
                }),
            )
            .await
            .expect("feed direct PCM");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let deadline = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(deadline);
    let mut provider_event_types = Vec::new();
    let mut saw_input_transcript = false;
    let mut saw_turn_done = false;
    let mut direct_audio_frames = 0_u64;
    loop {
        tokio::select! {
            _ = &mut deadline => panic!(
                "direct Codex Voice routing timed out: events={provider_event_types:?} input_transcript={saw_input_transcript} turn_done={saw_turn_done} audio_frames={direct_audio_frames}"
            ),
            event = helper.next_event() => {
                let event = event.expect("voice helper event");
                match event["type"].as_str().unwrap_or_default() {
                    "error" => panic!("voice helper failed: {}", event["message"]),
                    "pcm" if saw_input_transcript => direct_audio_frames += 1,
                    "data" => {
                        let message = &event["message"];
                        if let Some(kind) = message["type"].as_str()
                            && !provider_event_types.iter().any(|known| known == kind)
                        {
                            provider_event_types.push(kind.to_owned());
                        }
                        assert!(
                            parse_provider_delegation(message)
                                .expect("provider event")
                                .is_none(),
                            "self-contained identity question was delegated: {message}"
                        );
                        match message["type"].as_str() {
                            Some("input_transcript.added") => saw_input_transcript = true,
                            Some("turn.done") if saw_input_transcript => saw_turn_done = true,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        if saw_input_transcript && saw_turn_done && direct_audio_frames > 0 {
            break;
        }
    }

    stop_feed.store(true, Ordering::Release);
    feed.await.expect("direct PCM feed");
    helper.shutdown().await.expect("shutdown helper");
    eprintln!(
        "Direct Codex Voice routing: events={provider_event_types:?} audio_frames={direct_audio_frames}"
    );
}

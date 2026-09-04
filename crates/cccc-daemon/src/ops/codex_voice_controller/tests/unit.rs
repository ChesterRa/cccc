use super::super::projection::{SpeakableProgress, utf8_chunks};
use super::super::provider::{
    REALTIME_INSTRUCTIONS, configured_auth_path, validated_realtime_offer,
};
use super::super::*;
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn provider_delegations_are_strict_and_unrelated_events_are_ignored() {
    assert!(
        parse_provider_delegation(&json!({"type":"turn.done"}))
            .expect("unrelated provider event")
            .is_none()
    );
    assert!(
        parse_provider_delegation(&json!({
            "type":"delegation.created",
            "item":{"type":"delegation","target":"server","id":"d-1","content":[]}
        }))
        .expect("non-client delegation")
        .is_none()
    );
    assert_eq!(
        parse_provider_delegation(&json!({
            "type":"delegation.created",
            "item":{
                "type":"delegation", "target":"client", "id":"d-1",
                "content":[
                    {"type":"input_text","text":"inspect "},
                    {"type":"ignored","text":"wrong"},
                    {"type":"input_text","text":"the repository"}
                ]
            }
        }))
        .expect("valid provider delegation"),
        Some(ProviderDelegation {
            id: "d-1".into(),
            text: "inspect the repository".into(),
        })
    );
    assert!(
        parse_provider_delegation(&json!({
            "type":"delegation.created",
            "item":{"type":"delegation","target":"client","id":"","content":[]}
        }))
        .is_err()
    );
}

#[test]
fn context_chunks_are_utf8_safe_and_bounded() {
    let input = format!("{}界{}", "a".repeat(499), "b".repeat(501));
    let chunks = utf8_chunks(&input, 500);
    assert_eq!(chunks.concat(), input);
    assert!(chunks.iter().all(|chunk| chunk.len() <= 500));
}

#[test]
fn realtime_offer_validation_preserves_wire_bytes() {
    let offer = "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n";
    assert_eq!(
        validated_realtime_offer(offer).expect("valid SDP offer"),
        offer
    );
    assert!(validated_realtime_offer(" \r\n\t").is_err());
}

#[test]
fn realtime_voice_selection_is_allowlisted_and_normalized() {
    assert_eq!(validate_realtime_voice(" Cove ").expect("Cove"), "cove");
    assert_eq!(REALTIME_VOICES.len(), 9);
    assert!(validate_realtime_voice("arbitrary-provider-value").is_err());
    assert!(validate_realtime_voice("").is_err());
}

#[test]
fn realtime_credentials_follow_only_the_host_identity() {
    assert_eq!(
        configured_auth_path(None, Some("/host/codex".into())),
        Some(std::path::PathBuf::from("/host/codex/auth.json"))
    );
    assert_eq!(
        configured_auth_path(
            Some("/explicit/auth.json".into()),
            Some("/host/codex".into()),
        ),
        Some(std::path::PathBuf::from("/explicit/auth.json"))
    );
    assert_eq!(configured_auth_path(None, None), None);
}

#[test]
fn realtime_instructions_pin_direct_and_delegated_routing_boundaries() {
    for direct_case in [
        "greetings",
        "reactions",
        "jokes",
        "opinions",
        "identity or role questions",
        "clarifications",
        "self-contained discussion",
    ] {
        assert!(
            REALTIME_INSTRUCTIONS.contains(direct_case),
            "missing direct-answer boundary: {direct_case}"
        );
    }
    for delegated_case in [
        "current CCCC",
        "repository",
        "web or another external source",
        "a tool or operation",
        "an action",
        "substantial reasoning",
    ] {
        assert!(
            REALTIME_INSTRUCTIONS.contains(delegated_case),
            "missing delegation boundary: {delegated_case}"
        );
    }
    assert!(REALTIME_INSTRUCTIONS.contains("DO NOT delegate merely"));
    assert!(REALTIME_INSTRUCTIONS.contains("filler, a partial thought"));
    assert!(REALTIME_INSTRUCTIONS.contains("DELEGATE only a complete request"));
    assert!(REALTIME_INSTRUCTIONS.contains("Do not hold or discard it"));
    assert!(REALTIME_INSTRUCTIONS.contains("Runtime decides whether the input steers"));
}

#[test]
fn speakable_progress_waits_for_useful_multilingual_boundaries() {
    let mut progress = SpeakableProgress::default();
    assert!(
        progress
            .push("查到了第一处。 ")
            .expect("first sentence")
            .is_empty()
    );
    assert_eq!(
        progress
            .push("第二处也已确认！下一段尚未完成")
            .expect("second sentence"),
        vec!["查到了第一处。 第二处也已确认！"]
    );
    assert!(progress.push("。\n").expect("no paragraph yet").is_empty());
    assert_eq!(
        progress.push("\n继续调查。").expect("paragraph"),
        vec!["下一段尚未完成。"]
    );
    assert_eq!(
        progress.finish("查到了第一处。 第二处也已确认！下一段尚未完成。\n\n继续调查。"),
        vec!["继续调查。"]
    );
}

#[test]
fn final_projection_reconciles_authoritative_text_instead_of_trusting_deltas() {
    let progress_text = "Checking one source. Checking another source.";
    for final_text in [
        "The authoritative answer is 73.",
        "Correction: the answer is 74, not 73.",
    ] {
        let mut progress = SpeakableProgress::default();
        assert_eq!(
            progress.push(progress_text).expect("project test progress"),
            vec![progress_text]
        );
        assert_eq!(progress.finish(final_text), vec![final_text]);
    }
    let mut progress = SpeakableProgress::default();
    progress.push(progress_text).expect("project test progress");
    progress
        .push(" Preliminary, incomplete conclusion")
        .expect("project test progress");
    assert_eq!(
        progress.finish("Final conclusion."),
        vec!["Final conclusion."]
    );
}

#[test]
fn progress_history_is_bounded_even_when_paragraphs_are_continuously_flushed() {
    let mut progress = SpeakableProgress::default();
    progress
        .push("First sentence. Second sentence.")
        .expect("project test progress");
    for _ in 0..30 {
        progress
            .push(&format!("{}\n\n", "a".repeat(1000)))
            .expect("project test progress");
    }
    assert!(progress.push(&"a".repeat(4000)).is_err());
    assert_eq!(
        progress.finish("A short authoritative final."),
        vec!["A short authoritative final."]
    );
}

#[test]
fn final_projection_keeps_new_suffixes_but_does_not_repeat_delivered_results() {
    let mut progress = SpeakableProgress::default();
    progress
        .push("结果一。结果二。")
        .expect("project test progress");
    assert_eq!(
        progress.finish("结果一。结果二。结果三。"),
        vec!["结果三。"]
    );

    let mut progress = SpeakableProgress::default();
    progress
        .push("调查进度一。调查进度二。\n\n最终答案。\n\n")
        .expect("project test progress");
    assert!(
        progress
            .finish("调查进度一。调查进度二。\n\n最终答案。")
            .is_empty()
    );

    let mut progress = SpeakableProgress::default();
    progress
        .push("调查进度一。调查进度二。\n\n最终答案。")
        .expect("project test progress");
    assert_eq!(progress.finish("最终答案。"), vec!["最终答案。"]);
}

#[test]
fn codex_voice_uses_the_existing_global_recording_lease() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let voice_secretary = voice_recording_lease::update(
        &home,
        "g_one",
        "One",
        &json!({"action":"acquire","owner_id":"voice-secretary"}),
    )
    .expect("Voice Secretary lease");
    assert!(CallLease::acquire(&home, "g_two", "Two", "codex-voice:test").is_err());
    voice_recording_lease::release(
        &home,
        "g_one",
        "voice-secretary",
        voice_secretary["lease_id"].as_str().expect("lease id"),
    )
    .expect("release Voice Secretary");

    let codex_voice =
        CallLease::acquire(&home, "g_two", "Two", "codex-voice:test").expect("Codex Voice");
    codex_voice.heartbeat().expect("heartbeat");
    assert!(
        voice_recording_lease::update(
            &home,
            "g_one",
            "One",
            &json!({"action":"acquire","owner_id":"voice-secretary"}),
        )
        .is_err()
    );
    codex_voice.release().expect("release Codex Voice");
    assert_eq!(
        voice_recording_lease::current(&home).expect("recording lease state"),
        json!({})
    );
}

#[tokio::test]
async fn realtime_answer_rejects_an_oversized_chunked_body_while_streaming() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut request = vec![0_u8; 16 * 1024];
        let _ = stream.read(&mut request).await;
        stream
            .write_all(
                b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nContent-Type: application/sdp\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("response headers");
        let chunk = vec![b'a'; 16 * 1024];
        for _ in 0..17 {
            if stream
                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                .await
                .is_err()
                || stream.write_all(&chunk).await.is_err()
                || stream.write_all(b"\r\n").await.is_err()
            {
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    let temp = tempfile::tempdir().expect("tempdir");
    let auth_path = temp.path().join("auth.json");
    std::fs::write(
        &auth_path,
        json!({"tokens":{"access_token":"token","account_id":"account"}}).to_string(),
    )
    .expect("auth fixture");

    let error = create_realtime_answer(
        &RealtimeCallConfig {
            auth_path,
            base_url: format!("http://{address}"),
            voice: "cove".into(),
        },
        "v=0\r\n",
    )
    .await
    .expect_err("oversized response");
    assert!(error.to_string().contains("answer SDP is oversized"));
    server.await.expect("provider server");
}

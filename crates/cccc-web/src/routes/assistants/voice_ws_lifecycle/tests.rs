use super::*;

#[test]
fn only_document_capture_persists_secretary_artifacts() {
    assert!(persists_secretary_artifacts(
        &json!({"capture_mode":"document","dispatch_target":"document"})
    ));
    assert!(persists_secretary_artifacts(
        &json!({"dispatch_target":"document"})
    ));
    assert!(!persists_secretary_artifacts(
        &json!({"capture_mode":"document","dispatch_target":"composer"})
    ));
    assert!(!persists_secretary_artifacts(
        &json!({"capture_mode":"prompt","dispatch_target":"prompt"})
    ));
    assert!(!persists_secretary_artifacts(
        &json!({"capture_mode":"instruction","dispatch_target":"instruction"})
    ));
}

#[test]
fn recording_start_must_match_nonempty_lease_scope() {
    let lease = json!({"capture_mode":"prompt","dispatch_target":"composer"});
    assert!(
        validate_recording_lease_scope(
            &lease,
            &json!({"capture_mode":"prompt","dispatch_target":"composer"})
        )
        .is_ok()
    );
    let error = validate_recording_lease_scope(
        &lease,
        &json!({"capture_mode":"document","dispatch_target":"document"}),
    )
    .expect_err("mismatched start must be rejected");
    assert_eq!(error.code, "assistant_voice_recording_lease_mismatch");
    assert!(
        validate_recording_lease_scope(
            &json!({}),
            &json!({"capture_mode":"document","dispatch_target":"document"})
        )
        .is_ok()
    );
}

#[test]
fn stable_client_session_id_is_preserved() {
    assert_eq!(effective_session_id("voice-session"), "voice-session");
    assert!(effective_session_id("").starts_with("ws_"));
}

#[test]
fn final_asr_wins_but_streaming_text_is_a_safe_fallback() {
    assert_eq!(
        best_transcript(
            &json!({"ok":true,"text":"offline final"}),
            "streaming final".into()
        ),
        "offline final"
    );
    assert_eq!(
        best_transcript(
            &json!({"ok":false,"text":"failed output"}),
            "streaming final".into()
        ),
        "streaming final"
    );
    assert_eq!(
        best_transcript(&json!({"ok":true,"text":"  "}), "streaming final".into()),
        "streaming final"
    );
}

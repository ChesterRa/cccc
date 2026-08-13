use super::*;

#[test]
fn session_preview_pruning_keeps_the_newest_fifty_records() {
    let mut sessions = (0..51)
        .map(|index| {
            json!({
                "session_id":format!("session-{index:02}"),
                "updated_at":format!("2026-08-10T00:{index:02}:00Z")
            })
        })
        .collect::<Vec<_>>();

    prune_sessions(&mut sessions);

    assert_eq!(sessions.len(), 50);
    assert!(
        sessions
            .iter()
            .all(|session| session["session_id"] != "session-00")
    );
    assert!(
        sessions
            .iter()
            .any(|session| session["session_id"] == "session-50")
    );
}

#[test]
fn legacy_semantic_input_sessions_are_hidden() {
    let sessions = vec![json!({
        "session_id":"input-legacy",
        "document_path":"docs/voice.md",
        "segments":[{"text":"Target: composer", "trigger":{"trigger_kind":"user_instruction"}}]
    })];

    assert!(latest_document_session(&sessions, "docs/voice.md").is_none());
}

#[test]
fn semantic_segments_are_removed_from_mixed_legacy_sessions() {
    let sessions = vec![json!({
        "session_id":"meeting-1",
        "document_path":"docs/voice.md",
        "segments":[
            {"segment_id":"asr","text":"会议内容","is_final":true,"trigger":{"trigger_kind":"service_transcript"}},
            {"segment_id":"prompt","text":"优化提示词","is_final":true,"trigger":{"capture_mode":"prompt"}}
        ]
    })];

    let session = latest_document_session(&sessions, "docs/voice.md").expect("session");
    assert_eq!(session["segments"].as_array().map(Vec::len), Some(1));
    assert_eq!(session["transcript"], "会议内容");
    assert_eq!(session["capture_mode"], "document");
}

#[test]
fn latest_session_honors_the_requested_document() {
    let sessions = vec![
        json!({"session_id":"first","capture_mode":"document","document_path":"docs/first.md","segments":[{"text":"first"}]}),
        json!({"session_id":"second","capture_mode":"document","document_path":"docs/second.md","segments":[{"text":"second"}]}),
    ];

    assert_eq!(
        latest_document_session(&sessions, "docs/first.md").expect("first")["session_id"],
        "first"
    );
}

#[test]
fn legacy_midpoint_speaker_labels_are_not_restored() {
    let sessions = vec![json!({
        "session_id":"meeting-legacy",
        "capture_mode":"document",
        "document_path":"docs/voice.md",
        "segments":[{"text":"多人会议原始整段","start_ms":0,"end_ms":10000}],
        "diarization":{
            "model_id":"sherpa_onnx_diarization_pyannote_3dspeaker_zh",
            "speaker_transcript_model_id":"sherpa_onnx_diarization_pyannote_3dspeaker_zh",
            "speaker_transcript_segments":[{
                "text":"多人会议原始整段","start_ms":0,"end_ms":10000,
                "speaker_label":"Speaker 12","speaker_index":11
            }]
        }
    })];

    let session = latest_document_session(&sessions, "docs/voice.md").expect("session");
    assert!(
        session["diarization"]["speaker_transcript_segments"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert!(session["segments"][0].get("speaker_label").is_none());
}

#[test]
fn reads_legacy_python_window_segments() {
    let session = sanitize_document_session(
        &json!({
            "session_id":"python-session",
            "document_path":"docs/voice.md",
            "window_segments":[{"segment_id":"one","text":"hello"}]
        }),
        "docs/voice.md",
    )
    .expect("session");
    assert_eq!(session["capture_mode"], "document");
    assert_eq!(session["segments"][0]["text"], "hello");
}

#[test]
fn requested_document_filters_a_shared_python_session_even_after_path_changes() {
    let session = sanitize_document_session(
        &json!({
            "session_id":"python-shared-session",
            "capture_mode":"document",
            "document_path":"docs/b.md",
            "segments":[
                {"segment_id":"a","document_path":"docs/a.md","text":"alpha"},
                {"segment_id":"b","document_path":"docs/b.md","text":"bravo"}
            ]
        }),
        "docs/a.md",
    )
    .expect("session");

    assert_eq!(session["document_path"], "docs/a.md");
    assert_eq!(session["segments"].as_array().map(Vec::len), Some(1));
    assert_eq!(session["segments"][0]["segment_id"], "a");
    assert_eq!(session["transcript"], "alpha");
}

use serde_json::{Value, json};

pub(super) fn latest_document_session(sessions: &[Value], document_path: &str) -> Option<Value> {
    sessions
        .iter()
        .rev()
        .find_map(|session| sanitize_document_session(session, document_path))
}

pub(super) fn document_session_by_id(sessions: &[Value], session_id: &str) -> Option<Value> {
    sessions
        .iter()
        .find(|session| session["session_id"].as_str() == Some(session_id))
        .and_then(|session| sanitize_document_session(session, ""))
}

pub(super) fn clear_latest_document_session(sessions: &mut [Value], document_path: &str) -> bool {
    let Some(index) = sessions
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, session)| {
            sanitize_document_session(session, document_path).map(|_| index)
        })
    else {
        return false;
    };
    let session = &mut sessions[index];
    session["segments"] = json!([]);
    session["transcript"] = json!("");
    session["latest_partial"] = json!("");
    if let Some(diarization) = session
        .get_mut("diarization")
        .and_then(Value::as_object_mut)
    {
        diarization.insert("speaker_transcript_segments".into(), json!([]));
    }
    true
}

fn sanitize_document_session(session: &Value, requested_path: &str) -> Option<Value> {
    let capture_mode = normalized(session.get("capture_mode"));
    if !capture_mode.is_empty() && capture_mode != "document" {
        return None;
    }
    let session_id = session["session_id"].as_str().unwrap_or("");
    if capture_mode.is_empty()
        && (session_id.starts_with("input-")
            || matches!(
                session_id,
                "voice-secretary-prompt-refine" | "voice-secretary-user-instruction"
            ))
    {
        return None;
    }

    let raw_segments = session["segments"].as_array().cloned().unwrap_or_default();
    let segments = raw_segments
        .into_iter()
        .filter(is_document_transcript_segment)
        .collect::<Vec<_>>();
    let explicit_document_session = capture_mode == "document";
    if !explicit_document_session && segments.is_empty() {
        return None;
    }

    let document_path = session["document_path"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            segments.iter().find_map(|segment| {
                segment["document_path"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        })
        .unwrap_or_default();
    let requested_path = requested_path.trim();
    if !requested_path.is_empty() && document_path != requested_path {
        return None;
    }

    let mut sanitized = session.clone();
    sanitized["capture_mode"] = json!("document");
    sanitized["segments"] = json!(segments);
    sanitized["transcript"] = json!(
        sanitized["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|segment| segment["is_final"].as_bool().unwrap_or(true))
            .filter_map(|segment| segment["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    if let Some(diarization) = sanitized
        .get_mut("diarization")
        .and_then(Value::as_object_mut)
    {
        let speaker_segments = if has_windowed_speaker_transcript(diarization) {
            diarization
                .get("speaker_transcript_segments")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(is_document_transcript_segment)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        diarization.insert(
            "speaker_transcript_segments".into(),
            json!(speaker_segments),
        );
    }
    Some(sanitized)
}

fn has_windowed_speaker_transcript(diarization: &serde_json::Map<String, Value>) -> bool {
    let transcript_model = normalized(diarization.get("speaker_transcript_model_id"));
    let diarization_model = normalized(diarization.get("model_id"));
    !transcript_model.is_empty()
        && transcript_model != diarization_model
        && !transcript_model.contains("diarization")
        && !transcript_model.contains("pyannote")
        && !transcript_model.contains("3dspeaker")
}

fn is_document_transcript_segment(segment: &Value) -> bool {
    if segment["text"]
        .as_str()
        .is_none_or(|text| text.trim().is_empty())
    {
        return false;
    }
    let trigger = segment.get("trigger").unwrap_or(&Value::Null);
    let semantic_values = [
        segment.get("kind"),
        segment.get("input_kind"),
        segment.get("capture_mode"),
        trigger.get("kind"),
        trigger.get("input_kind"),
        trigger.get("capture_mode"),
        trigger.get("mode"),
        trigger.get("trigger_kind"),
        trigger.get("source"),
        trigger.get("dispatch_target"),
        trigger.get("target_kind"),
    ];
    !semantic_values.into_iter().flatten().any(|value| {
        matches!(
            normalized(Some(value)).as_str(),
            "prompt"
                | "prompt_refine"
                | "composer_prompt_refine"
                | "instruction"
                | "voice_instruction"
                | "user_instruction"
                | "composer"
        )
    })
}

fn normalized(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

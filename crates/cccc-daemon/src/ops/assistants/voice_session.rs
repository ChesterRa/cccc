use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, assistant_state};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

use super::{voice_document_state, voice_input};

const MAX_SESSION_RECORDS: usize = 50;

pub(super) fn view(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let session_id = string_arg(request, "session_id").unwrap_or_default();
    let document_path = string_arg(request, "document_path").unwrap_or_default();
    if session_id.is_empty()
        && let Some(session) =
            document_transcript_session(home, &group_id, &document_path).map_err(OpError::io)?
    {
        return object(json!({"group_id":group_id,"session":session}));
    }
    let state = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let sessions = state["sessions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let session = if session_id.is_empty() {
        latest_document_session(sessions, &document_path)
    } else {
        document_session_by_id(sessions, &session_id, &document_path)
    };
    object(json!({"group_id":group_id,"session":session.unwrap_or_else(||json!({}))}))
}

fn document_transcript_session(
    home: &HomeLayout,
    group_id: &str,
    document_path: &str,
) -> std::io::Result<Option<Value>> {
    let Some((document_id, path)) =
        voice_document_state::transcript_log(home, group_id, document_path)?
    else {
        return Ok(None);
    };
    let mut segments = voice_input::read_jsonl_matching(&path, |segment| {
        segment["text"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
    })?;
    if segments.len() > 1_000 {
        segments.drain(..segments.len() - 1_000);
    }
    let Some(first) = segments.first() else {
        return Ok(None);
    };
    let last = segments.last().expect("non-empty transcript segments");
    let transcript = segments
        .iter()
        .filter(|segment| segment["is_final"].as_bool().unwrap_or(true))
        .filter_map(|segment| segment["text"].as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(json!({
        "schema":1,
        "group_id":group_id,
        "session_id":format!("document-{document_id}"),
        "status":"ready",
        "capture_mode":"document",
        "document_path":document_path,
        "created_at":first["created_at"].as_str().unwrap_or_default(),
        "updated_at":last["updated_at"].as_str().or_else(||last["created_at"].as_str()).unwrap_or_default(),
        "segments":segments,
        "transcript":transcript,
        "diarization":{},
        "source":"document_transcript"
    })))
}

pub(super) fn clear_transcript(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let requested_session_id = match string_arg(request, "session_id") {
        Some(session_id) if !session_id.trim().is_empty() => voice_input::safe_id(&session_id)?,
        _ => String::new(),
    };
    let requested_document_path = string_arg(request, "document_path").unwrap_or_default();
    let (cleared, session_id) = voice_input::with_transcript_lock(home, &group_id, || {
        let (mut cleared, session_id, document_path) =
            assistant_state::update(home, &group_id, |state| {
                let sessions = state
                    .entry("sessions")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .expect("assistant sessions initialized");
                let index = if requested_session_id.is_empty() {
                    latest_document_session_index(sessions, &requested_document_path)
                } else {
                    sessions.iter().position(|session| {
                        session["session_id"].as_str() == Some(requested_session_id.as_str())
                            && sanitize_document_session(session, &requested_document_path)
                                .is_some()
                    })
                };
                let Some(index) = index else {
                    return Ok((
                        false,
                        requested_session_id.clone(),
                        requested_document_path.clone(),
                    ));
                };
                let session = &mut sessions[index];
                let session_id = session["session_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let document_path = if requested_document_path.is_empty() {
                    session["document_path"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                } else {
                    requested_document_path.clone()
                };
                session["segments"] = json!([]);
                session["transcript"] = json!("");
                session["latest_partial"] = json!("");
                session["window_text"] = json!("");
                session["window_segments"] = json!([]);
                session["window_segment_count"] = json!(0);
                session["window_first_segment_id"] = json!("");
                session["window_last_segment_id"] = json!("");
                let now = cccc_contracts::utc_now();
                session["transcript_cleared_at"] = json!(now);
                session["updated_at"] = json!(now);
                if let Some(diarization) = session
                    .get_mut("diarization")
                    .and_then(Value::as_object_mut)
                {
                    diarization.insert("speaker_transcript_segments".into(), json!([]));
                }
                Ok((true, session_id, document_path))
            })?;

        if !session_id.is_empty() {
            cleared |= remove_if_present(
                &home
                    .root()
                    .join("voice-secretary")
                    .join(&group_id)
                    .join(&session_id)
                    .join("transcripts/segments.jsonl"),
            )?;
        }
        if !document_path.is_empty() {
            let documents = voice_document_state::load(home, &group_id)?;
            if let Some(document_id) = documents["documents"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|document| document["document_path"] == document_path)
                .and_then(|document| document["document_id"].as_str())
            {
                let document_id = voice_input::safe_id(document_id)
                    .map_err(|error| std::io::Error::other(error.message))?;
                cleared |= remove_if_present(
                    &home
                        .root()
                        .join("voice-secretary")
                        .join(&group_id)
                        .join("documents")
                        .join(document_id)
                        .join("transcript.jsonl"),
                )?;
            }
        }
        Ok((cleared, session_id))
    })
    .map_err(OpError::io)?;
    object(json!({"group_id":group_id,"session_id":session_id,"cleared":cleared}))
}

pub(super) fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let session_id = voice_input::safe_id(&required_arg(request, "session_id")?)?;
    let patch = request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let allowed = [
        "status",
        "capture_mode",
        "document_path",
        "audio_duration_ms",
        "diarization_ready",
        "diarization_artifact_path",
        "diarization",
        "diarization_error",
        "error",
        "latest_partial",
    ];
    let session = assistant_state::update(home, &group_id, |state| {
        let sessions = state
            .entry("sessions")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("assistant sessions initialized");
        let index = sessions
            .iter()
            .position(|session| session["session_id"] == session_id)
            .unwrap_or_else(|| {
                sessions.push(json!({
                    "schema":1,"group_id":group_id,"session_id":session_id,
                    "capture_mode":"document","segments":[],"transcript":"",
                    "created_at":cccc_contracts::utc_now()
                }));
                sessions.len() - 1
            });
        {
            let session = &mut sessions[index];
            for key in allowed {
                if let Some(value) = patch.get(key) {
                    session[key] = value.clone();
                }
            }
            if session["diarization_ready"].as_bool() == Some(true)
                && let Some(session) = session.as_object_mut()
            {
                session.remove("diarization_error");
                if session.get("error").is_some_and(Value::is_null) {
                    session.remove("error");
                }
            }
            session["schema"] = json!(1);
            session["group_id"] = json!(group_id);
            session["session_id"] = json!(session_id);
            session["capture_mode"] = json!("document");
            session["updated_at"] = json!(cccc_contracts::utc_now());
        }
        prune_sessions(sessions);
        Ok(sessions
            .iter()
            .find(|session| session["session_id"] == session_id)
            .cloned()
            .expect("updated session retained"))
    })
    .map_err(OpError::io)?;
    object(json!({
        "group_id":group_id,
        "session":sanitize_document_session(&session, "").unwrap_or(session)
    }))
}

pub(super) fn prune_sessions(sessions: &mut Vec<Value>) {
    while sessions.len() > MAX_SESSION_RECORDS {
        let oldest = sessions
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                session_order(left).cmp(session_order(right)).then_with(|| {
                    left["session_id"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["session_id"].as_str().unwrap_or_default())
                })
            })
            .map(|(index, _)| index)
            .expect("non-empty session records");
        sessions.remove(oldest);
    }
}

fn session_order(session: &Value) -> &str {
    session["updated_at"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| session["created_at"].as_str())
        .unwrap_or_default()
}

fn remove_if_present(path: &std::path::Path) -> std::io::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn latest_document_session(sessions: &[Value], document_path: &str) -> Option<Value> {
    sessions
        .iter()
        .rev()
        .find_map(|session| sanitize_document_session(session, document_path))
}

fn latest_document_session_index(sessions: &[Value], document_path: &str) -> Option<usize> {
    sessions
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, session)| {
            sanitize_document_session(session, document_path).map(|_| index)
        })
}

fn document_session_by_id(
    sessions: &[Value],
    session_id: &str,
    document_path: &str,
) -> Option<Value> {
    sessions
        .iter()
        .find(|session| session["session_id"].as_str() == Some(session_id))
        .and_then(|session| sanitize_document_session(session, document_path))
}

fn sanitize_document_session(session: &Value, requested_path: &str) -> Option<Value> {
    let capture_mode = normalized(session.get("capture_mode"));
    if !capture_mode.is_empty() && capture_mode != "document" {
        return None;
    }
    let session_id = session["session_id"].as_str().unwrap_or("");
    if session_id.is_empty()
        || voice_input::safe_id(session_id).is_err()
        || (capture_mode.is_empty()
            && (session_id.starts_with("input-")
                || matches!(
                    session_id,
                    "voice-secretary-prompt-refine" | "voice-secretary-user-instruction"
                )))
    {
        return None;
    }
    let source_segments = session["segments"]
        .as_array()
        .or_else(|| session["window_segments"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut segments = source_segments
        .into_iter()
        .filter(is_document_transcript_segment)
        .collect::<Vec<_>>();
    let mut document_path = session["document_path"]
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
    if !requested_path.is_empty() {
        segments.retain(|segment| {
            let segment_path = segment["document_path"]
                .as_str()
                .map(str::trim)
                .unwrap_or_default();
            if segment_path.is_empty() {
                document_path.is_empty() || document_path == requested_path
            } else {
                segment_path == requested_path
            }
        });
        if document_path != requested_path && segments.is_empty() {
            return None;
        }
        document_path = requested_path.to_owned();
    }
    if capture_mode.is_empty() && segments.is_empty() {
        return None;
    }
    let mut sanitized = session.clone();
    sanitized["capture_mode"] = json!("document");
    sanitized["document_path"] = json!(document_path);
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
mod tests;

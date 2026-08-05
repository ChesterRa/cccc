use cccc_contracts::{Event, utc_now};
use cccc_core::{GroupStore, integration_state, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{STORE_KEY, root, voice_asr, voice_inference};
use crate::AppState;

pub(super) enum SpawnStatus {
    Started,
    Skipped(&'static str),
}

pub(super) fn spawn(
    state: AppState,
    group_id: String,
    session_id: String,
    document_path: String,
    model: String,
    recording: tempfile::NamedTempFile,
) -> SpawnStatus {
    if !voice_asr::diarization_available(&state.home, &model) {
        return SpawnStatus::Skipped("model_not_ready");
    }
    let Some(permit) = voice_inference::try_acquire() else {
        return SpawnStatus::Skipped("worker_busy");
    };
    tokio::spawn(async move {
        let home = state.home.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            voice_asr::diarize_pcm16_file(&home, &model, recording.path(), 16_000)
        })
        .await;
        drop(permit);
        let (action, result, error_code, error_message) = match outcome {
            Ok(Ok(Some(result))) => ("diarization_ready", Some(result), "", String::new()),
            Ok(Ok(None)) => (
                "diarization_failed",
                None,
                "diarization_model_unavailable",
                "speaker diarization model became unavailable".into(),
            ),
            Ok(Err(error)) => ("diarization_failed", None, error.code, error.message),
            Err(error) => (
                "diarization_failed",
                None,
                "diarization_task_failed",
                error.to_string(),
            ),
        };
        if result.is_some() {
            wait_for_transcript(&state, &group_id, &session_id).await;
        }
        if persist_result(
            &state,
            &group_id,
            &session_id,
            &document_path,
            result,
            error_code,
            &error_message,
        )
        .is_err()
        {
            return;
        }
        if let Err(error) = emit_event_with_retry(
            &state,
            &group_id,
            &session_id,
            &document_path,
            action,
            error_code,
            &error_message,
        )
        .await
        {
            tracing::error!(
                %error,
                %group_id,
                %session_id,
                "failed to emit voice diarization completion event"
            );
        }
    });
    SpawnStatus::Started
}

async fn wait_for_transcript(state: &AppState, group_id: &str, session_id: &str) {
    for _ in 0..20 {
        if session_segments(state, group_id, session_id).is_some_and(|items| !items.is_empty()) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn session_segments(state: &AppState, group_id: &str, session_id: &str) -> Option<Vec<Value>> {
    let store = GroupStore::new(state.home.clone()).ok()?;
    let value = integration_state::group_get(&store, group_id, STORE_KEY).ok()?;
    value["sessions"]
        .as_array()?
        .iter()
        .find(|item| item["session_id"] == session_id)?["segments"]
        .as_array()
        .cloned()
}

fn persist_result(
    state: &AppState,
    group_id: &str,
    session_id: &str,
    document_path: &str,
    result: Option<Value>,
    error_code: &str,
    error_message: &str,
) -> std::io::Result<()> {
    let store = GroupStore::new(state.home.clone())?;
    integration_state::group_update(&store, group_id, STORE_KEY, |value| {
        let sessions = root(value)
            .entry("sessions")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("sessions initialized");
        let index = sessions
            .iter()
            .position(|item| item["session_id"] == session_id)
            .unwrap_or_else(|| {
                sessions.push(json!({
                    "session_id":session_id,"document_path":document_path,
                    "segments":[],"transcript":"","created_at":utc_now()
                }));
                sessions.len() - 1
            });
        let session = &mut sessions[index];
        session["updated_at"] = json!(utc_now());
        session["diarization_ready"] = json!(result.is_some());
        if let Some(mut result) = result {
            result["speaker_transcript_segments"] =
                json!(speaker_transcript_segments(&result, &session["segments"]));
            result["speaker_transcript_model_id"] = result["model_id"].clone();
            session["diarization"] = result;
            if let Some(session) = session.as_object_mut() {
                session.remove("diarization_error");
            }
        } else {
            session["diarization_error"] = json!({"code":error_code,"message":error_message});
        }
        Ok(())
    })
}

fn speaker_transcript_segments(diarization: &Value, transcript: &Value) -> Vec<Value> {
    let ranges = diarization["segments"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    transcript
        .as_array()
        .into_iter()
        .flatten()
        .map(|segment| {
            let mut segment = segment.clone();
            let start = segment["start_ms"].as_i64().unwrap_or(0);
            let end = segment["end_ms"].as_i64().unwrap_or(start);
            let midpoint = start.saturating_add(end).saturating_div(2);
            if let Some(range) = ranges.iter().find(|range| {
                range["start_ms"].as_i64().unwrap_or(i64::MAX) <= midpoint
                    && range["end_ms"].as_i64().unwrap_or(i64::MIN) >= midpoint
            }) {
                segment["speaker_index"] = range["speaker"].clone();
                segment["speaker_label"] = range["speaker_label"].clone();
            }
            segment
        })
        .collect()
}

async fn emit_event_with_retry(
    state: &AppState,
    group_id: &str,
    session_id: &str,
    document_path: &str,
    action: &str,
    error_code: &str,
    error_message: &str,
) -> std::io::Result<()> {
    let store = GroupStore::new(state.home.clone())?;
    let path = store.ledger_path(group_id)?;
    let mut event = Event::new("assistant.voice.session", group_id);
    event.id = format!(
        "{:x}",
        Sha256::digest(format!(
            "voice-diarization:{group_id}:{session_id}:{action}"
        ))
    );
    event.by = "system".into();
    event.data = json!({
        "action":action,"session_id":session_id,"document_path":document_path,
        "error_code":error_code,"error_message":error_message
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    let mut last_error = None;
    for attempt in 0..4 {
        if ledger::read_all(&path)
            .is_ok_and(|events| events.iter().any(|existing| existing.id == event.id))
        {
            return Ok(());
        }
        match ledger::append(&path, &event) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("event append failed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_ranges_are_applied_to_transcript_segments() {
        let result = json!({"segments":[
            {"start_ms":0,"end_ms":900,"speaker":0,"speaker_label":"Speaker 1"},
            {"start_ms":901,"end_ms":2000,"speaker":1,"speaker_label":"Speaker 2"}
        ]});
        let transcript = json!([
            {"segment_id":"one","text":"hello","start_ms":0,"end_ms":800},
            {"segment_id":"two","text":"world","start_ms":1000,"end_ms":1800}
        ]);
        let mapped = speaker_transcript_segments(&result, &transcript);
        assert_eq!(mapped[0]["speaker_index"], 0);
        assert_eq!(mapped[1]["speaker_label"], "Speaker 2");
    }
}

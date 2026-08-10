use cccc_contracts::{Event, utc_now};
use cccc_core::{GroupStore, integration_state, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{STORE_KEY, root, voice_asr, voice_inference, voice_speaker_transcript};
use crate::AppState;

pub(super) enum SpawnStatus {
    Started,
    Skipped(&'static str),
}

pub(super) struct DiarizationJob {
    pub(super) state: AppState,
    pub(super) group_id: String,
    pub(super) session_id: String,
    pub(super) document_path: String,
    pub(super) diarization_model: String,
    pub(super) transcript_model: String,
    pub(super) language: String,
}

pub(super) fn spawn(job: DiarizationJob, recording: tempfile::NamedTempFile) -> SpawnStatus {
    let DiarizationJob {
        state,
        group_id,
        session_id,
        document_path,
        diarization_model,
        transcript_model,
        language,
    } = job;
    if !voice_asr::diarization_available(&state.home, &diarization_model) {
        return SpawnStatus::Skipped("model_not_ready");
    }
    let Some(permit) = voice_inference::try_acquire() else {
        return SpawnStatus::Skipped("worker_busy");
    };
    tokio::spawn(async move {
        let home = state.home.clone();
        let outcome =
            tokio::task::spawn_blocking(move || -> Result<Option<Value>, voice_asr::VoiceError> {
                let Some(mut result) = voice_asr::diarize_pcm16_file(
                    &home,
                    &diarization_model,
                    recording.path(),
                    16_000,
                )?
                else {
                    return Ok(None);
                };
                voice_speaker_transcript::normalize_diarization_result(&mut result);
                let transcript = voice_speaker_transcript::build(
                    &home,
                    &transcript_model,
                    recording.path(),
                    &language,
                    &result,
                )?;
                result["speaker_transcript_segments"] = json!(transcript.segments);
                result["speaker_transcript_model_id"] = json!(transcript.model_id);
                Ok(Some(result))
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
                    "capture_mode":"document","segments":[],"transcript":"","created_at":utc_now()
                }));
                sessions.len() - 1
            });
        let session = &mut sessions[index];
        session["updated_at"] = json!(utc_now());
        session["capture_mode"] = json!("document");
        session["diarization_ready"] = json!(result.is_some());
        if let Some(result) = result {
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

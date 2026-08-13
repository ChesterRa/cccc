use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupStore, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    voice_asr, voice_inference, voice_segment_analysis, voice_segmented_recording::RecordingSegment,
};
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

pub(super) fn spawn(job: DiarizationJob, recordings: Vec<RecordingSegment>) -> SpawnStatus {
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
        let outcome = tokio::task::spawn_blocking(move || {
            voice_segment_analysis::analyze(
                &home,
                &diarization_model,
                &transcript_model,
                &language,
                &recordings,
            )
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
        if let Err(error) = persist_result(
            &state,
            &group_id,
            &session_id,
            &document_path,
            result,
            error_code,
            &error_message,
        )
        .await
        {
            tracing::error!(
                %error,
                %group_id,
                %session_id,
                "failed to persist voice diarization completion"
            );
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

async fn persist_result(
    state: &AppState,
    group_id: &str,
    session_id: &str,
    document_path: &str,
    result: Option<Value>,
    error_code: &str,
    error_message: &str,
) -> std::io::Result<()> {
    let response = state
        .client
        .call(&completion_request(
            group_id,
            session_id,
            document_path,
            result,
            error_code,
            error_message,
        ))
        .await
        .map_err(std::io::Error::other)?;
    if response.ok {
        Ok(())
    } else {
        Err(std::io::Error::other(
            response
                .error
                .map(|error| format!("{}: {}", error.code, error.message))
                .unwrap_or_else(|| "voice session update failed".into()),
        ))
    }
}

fn completion_request(
    group_id: &str,
    session_id: &str,
    document_path: &str,
    result: Option<Value>,
    error_code: &str,
    error_message: &str,
) -> DaemonRequest {
    let patch = if let Some(result) = result {
        json!({
            "status":"closed",
            "document_path":document_path,
            "diarization_ready":true,
            "diarization":result,
            "error":null
        })
    } else {
        json!({
            "status":"closed",
            "document_path":document_path,
            "diarization_ready":false,
            "diarization_error":{"code":error_code,"message":error_message},
            "error":{"code":error_code,"message":error_message}
        })
    };
    DaemonRequest {
        v: 1,
        op: "assistant_voice_session_update".into(),
        args: json!({
            "group_id":group_id,
            "session_id":session_id,
            "by":"assistant:voice_secretary",
            "patch":patch
        })
        .as_object()
        .cloned()
        .expect("voice session update args"),
    }
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
#[path = "voice_diarization/tests.rs"]
mod tests;

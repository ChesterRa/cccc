use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{
    voice_asr, voice_inference, voice_segment_analysis, voice_segmented_recording::RecordingSegment,
};
use crate::AppState;

static DIARIZATION_JOBS: OnceLock<Arc<Semaphore>> = OnceLock::new();

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

fn available(state: &AppState, model_id: &str) -> bool {
    voice_asr::diarization_available(&state.home, model_id)
}

pub(super) fn try_reserve(
    state: &AppState,
    model_id: &str,
) -> Result<OwnedSemaphorePermit, &'static str> {
    if !available(state, model_id) {
        return Err("model_not_ready");
    }
    reservation_semaphore()
        .try_acquire_owned()
        .map_err(|_| "busy")
}

pub(super) fn spawn(
    job: DiarizationJob,
    recordings: Vec<RecordingSegment>,
    reservation: OwnedSemaphorePermit,
) -> SpawnStatus {
    let DiarizationJob {
        state,
        group_id,
        session_id,
        document_path,
        diarization_model,
        transcript_model,
        language,
    } = job;
    tokio::spawn(async move {
        let _reservation = reservation;
        let home = state.home.clone();
        let permit = voice_inference::acquire().await;
        let outcome = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            voice_segment_analysis::analyze(
                &home,
                &diarization_model,
                &transcript_model,
                &language,
                &recordings,
            )
        })
        .await;
        let (result, error_code, error_message) = match outcome {
            Ok(Ok(Some(result))) => (Some(result), "", String::new()),
            Ok(Ok(None)) => (
                None,
                "diarization_model_unavailable",
                "speaker diarization model became unavailable".into(),
            ),
            Ok(Err(error)) => (None, error.code, error.message),
            Err(error) => (None, "diarization_task_failed", error.to_string()),
        };
        if let Err(error) = persist_result(
            &state.client,
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
        }
    });
    SpawnStatus::Started
}

fn reservation_semaphore() -> Arc<Semaphore> {
    DIARIZATION_JOBS
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

async fn persist_result(
    client: &cccc_client::DaemonClient,
    group_id: &str,
    session_id: &str,
    document_path: &str,
    result: Option<Value>,
    error_code: &str,
    error_message: &str,
) -> std::io::Result<()> {
    let request = completion_request(
        group_id,
        session_id,
        document_path,
        result,
        error_code,
        error_message,
    );
    for attempt in 0..4 {
        // The daemon owns both the projection and its deterministic completion
        // event. Retrying this one operation also covers an unknown IPC result.
        let error = match client.call(&request).await {
            Ok(response) if response.ok => {
                if response
                    .result
                    .get("completion_event_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.is_empty())
                {
                    return Ok(());
                }
                return Err(std::io::Error::other(
                    "daemon did not confirm the completion event; matching daemon and Web builds are required",
                ));
            }
            Ok(response) => {
                let error = response.error;
                let retryable = error.as_ref().is_some_and(|error| error.code == "io_error");
                let error = std::io::Error::other(
                    error
                        .map(|error| format!("{}: {}", error.code, error.message))
                        .unwrap_or_else(|| "voice session update failed".into()),
                );
                if !retryable {
                    return Err(error);
                }
                error
            }
            Err(error) => std::io::Error::other(error),
        };
        if attempt == 3 {
            return Err(error);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
    }
    unreachable!("bounded completion retry returns on its final attempt")
}

fn completion_request(
    group_id: &str,
    session_id: &str,
    document_path: &str,
    result: Option<Value>,
    error_code: &str,
    error_message: &str,
) -> DaemonRequest {
    let completion_event = if result.is_some() {
        "diarization_ready"
    } else {
        "diarization_failed"
    };
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
            "patch":patch,
            "completion_event":completion_event
        })
        .as_object()
        .cloned()
        .expect("voice session update args"),
    }
}

#[cfg(test)]
#[path = "voice_diarization/tests.rs"]
mod tests;

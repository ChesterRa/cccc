use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};

use super::{
    object, short_id, voice_asr, voice_diarization, voice_final_asr, voice_segmented_recording,
};
use crate::AppState;

/// Only document capture owns durable Voice Secretary artifacts. Prompt,
/// instruction, and direct-composer modes are dispatched by the browser and
/// must not create meeting sessions or speaker-analysis jobs.
pub(super) fn persists_secretary_artifacts(command: &Value) -> bool {
    command["capture_mode"].as_str().unwrap_or("document") == "document"
        && command["dispatch_target"].as_str() != Some("composer")
}

pub(super) fn validate_recording_lease_scope(
    lease: &Value,
    command: &Value,
) -> Result<(), voice_asr::VoiceError> {
    let leased_capture_mode = lease["capture_mode"].as_str().unwrap_or("").trim();
    let leased_dispatch_target = lease["dispatch_target"].as_str().unwrap_or("").trim();
    let requested_capture_mode = command["capture_mode"]
        .as_str()
        .unwrap_or("document")
        .trim();
    let requested_dispatch_target = command["dispatch_target"].as_str().unwrap_or("").trim();
    if (!leased_capture_mode.is_empty() && leased_capture_mode != requested_capture_mode)
        || (!leased_dispatch_target.is_empty()
            && leased_dispatch_target != requested_dispatch_target)
    {
        return Err(voice_asr::VoiceError::new(
            "assistant_voice_recording_lease_mismatch",
            "voice transcription mode does not match the active recording lease",
        ));
    }
    Ok(())
}

pub(super) struct DisconnectContext {
    pub(super) state: AppState,
    pub(super) group_id: String,
    pub(super) client_session_id: String,
    pub(super) document_path: String,
    pub(super) language: String,
    pub(super) final_model_id: String,
    pub(super) diarization_model_id: String,
    pub(super) persist_artifacts: bool,
}

pub(super) async fn finalize_disconnect(
    context: DisconnectContext,
    streaming: Option<voice_asr::StreamingSession>,
    recording: Option<voice_segmented_recording::SegmentedPcmRecording>,
) {
    let DisconnectContext {
        state,
        group_id,
        client_session_id,
        document_path,
        language,
        final_model_id,
        diarization_model_id,
        persist_artifacts,
    } = context;
    if !persist_artifacts {
        return;
    }
    let session_id = effective_session_id(&client_session_id);
    let streaming_text = finish_streaming(streaming).await;
    let Some(recordings) = finish_recording(recording).await else {
        persist_disconnect_text(
            &state,
            &group_id,
            &session_id,
            &document_path,
            &language,
            &streaming_text,
            "assistant_service_local_asr_streaming",
        )
        .await;
        return;
    };

    let diarization_reservation = voice_diarization::try_reserve(&state, &diarization_model_id);
    let can_defer_to_speaker_analysis = diarization_reservation.is_ok();
    let final_result = voice_final_asr::transcribe_pcm16_segments(
        state.home.clone(),
        final_model_id.clone(),
        language.clone(),
        &recordings,
        can_defer_to_speaker_analysis,
    )
    .await;
    let final_text = best_transcript(&final_result, streaming_text);

    persist_disconnect_text(
        &state,
        &group_id,
        &session_id,
        &document_path,
        &language,
        &final_text,
        "assistant_service_local_asr_final",
    )
    .await;
    if let Ok(reservation) = diarization_reservation {
        let _ = voice_diarization::spawn(
            voice_diarization::DiarizationJob {
                state,
                group_id,
                session_id,
                document_path,
                diarization_model: diarization_model_id,
                transcript_model: final_model_id,
                language,
            },
            recordings,
            reservation,
        );
    }
}

fn best_transcript(final_result: &Value, streaming_text: String) -> String {
    final_result["text"]
        .as_str()
        .filter(|_| final_result["ok"].as_bool().unwrap_or(false))
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(streaming_text)
}

async fn finish_streaming(streaming: Option<voice_asr::StreamingSession>) -> String {
    let Some(mut streaming) = streaming else {
        return String::new();
    };
    tokio::task::spawn_blocking(move || streaming.finish())
        .await
        .ok()
        .flatten()
        .and_then(|event| event["text"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

async fn finish_recording(
    recording: Option<voice_segmented_recording::SegmentedPcmRecording>,
) -> Option<Vec<voice_segmented_recording::RecordingSegment>> {
    let recording = recording.filter(|recording| !recording.is_empty())?;
    match recording.finish().await {
        Ok(recordings) if !recordings.is_empty() => Some(recordings),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(code = error.code, %error.message, "disconnected voice recording could not be finalized");
            None
        }
    }
}

async fn persist_disconnect_text(
    state: &AppState,
    group_id: &str,
    session_id: &str,
    document_path: &str,
    language: &str,
    text: &str,
    recognition_backend: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    let args = object(json!({
        "group_id":group_id,
        "by":"user",
        "session_id":session_id,
        "segment_id":"ws-disconnect-final",
        "text":text,
        "language":language,
        "document_path":document_path,
        "is_final":true,
        "flush":true,
        "start_ms":0,
        "trigger":{
            "trigger_kind":"websocket_disconnect",
            "capture_mode":"service",
            "recognition_backend":recognition_backend,
        }
    }));
    let result = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: "assistant_voice_transcript_append".into(),
            args,
        })
        .await;
    match result {
        Ok(response) if response.ok => {}
        Ok(response) => {
            tracing::warn!(?response.error, "final voice transcript was rejected during disconnect");
        }
        Err(error) => {
            tracing::warn!(%error, "final voice transcript could not be delivered during disconnect");
        }
    }
}

fn effective_session_id(client_session_id: &str) -> String {
    if client_session_id.trim().is_empty() {
        format!("ws_{}", short_id())
    } else {
        client_session_id.to_owned()
    }
}

#[cfg(test)]
mod tests;

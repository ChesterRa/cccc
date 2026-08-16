use cccc_core::HomeLayout;
use serde_json::{Value, json};

use super::{
    voice_asr, voice_diarization, voice_final_asr, voice_segmented_recording, voice_ws_lifecycle,
};
use crate::AppState;

pub(super) struct VoiceWsCapture {
    selected_model: String,
    diarization_model: String,
    streaming: Option<voice_asr::StreamingSession>,
    recording: Option<voice_segmented_recording::SegmentedPcmRecording>,
    client_session_id: String,
    document_path: String,
    language: String,
    persist_artifacts: bool,
    audio_seq: u64,
}

impl VoiceWsCapture {
    pub(super) fn new(assistant: &Value) -> Self {
        Self {
            selected_model: assistant["config"]["service_model_id"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
            diarization_model: assistant["config"]["service_diarization_model_id"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
            streaming: None,
            recording: None,
            client_session_id: String::new(),
            document_path: String::new(),
            language: String::new(),
            persist_artifacts: true,
            audio_seq: 0,
        }
    }

    pub(super) fn is_started(&self) -> bool {
        self.streaming.is_some() || self.recording.is_some()
    }

    pub(super) async fn start(
        &mut self,
        home: &HomeLayout,
        command: &Value,
    ) -> Result<Value, voice_asr::VoiceError> {
        if self.is_started() {
            return Err(voice_asr::VoiceError::new(
                "recording_already_started",
                "the active recording must be stopped before starting another one",
            ));
        }
        if command["sample_rate"].as_i64().unwrap_or(16_000) != 16_000 {
            return Err(voice_asr::VoiceError::new(
                "unsupported_sample_rate",
                "streaming ASR requires 16000 Hz PCM16",
            ));
        }
        let model_home = home.clone();
        let model_id = self.selected_model.clone();
        let opened = tokio::task::spawn_blocking(move || {
            voice_asr::StreamingSession::open(&model_home, &model_id)
        })
        .await
        .map_err(task_error)??;
        let recording = voice_segmented_recording::SegmentedPcmRecording::create(home)?;
        let actual_model_id = opened.model_id.clone();

        self.client_session_id = command["session_id"].as_str().unwrap_or("").to_owned();
        self.document_path = command["document_path"].as_str().unwrap_or("").to_owned();
        self.language = command["language"].as_str().unwrap_or("").to_owned();
        self.persist_artifacts = voice_ws_lifecycle::persists_secretary_artifacts(command);
        self.streaming = Some(opened);
        self.recording = Some(recording);
        Ok(json!({
            "type":"ready","ok":true,"seq":command["seq"],"sample_rate":16000,
            "audio_transport":"binary_pcm16","model_id":actual_model_id,
            "backend":"assistant_service_local_asr",
            "recording_segment_duration_ms":voice_segmented_recording::DEFAULT_SEGMENT_DURATION_MS
        }))
    }

    pub(super) async fn accept_audio(
        &mut self,
        bytes: &[u8],
    ) -> Result<Vec<Value>, voice_asr::VoiceError> {
        let recording = self.recording.as_mut().ok_or_else(|| {
            voice_asr::VoiceError::new("audio_before_start", "recording storage is not initialized")
        })?;
        let boundaries = recording.append(bytes).await?;
        let mut active = self.streaming.take().ok_or_else(|| {
            voice_asr::VoiceError::new(
                "audio_before_start",
                "binary audio received before the start command",
            )
        })?;
        let audio = bytes.to_vec();
        let decoded = tokio::task::spawn_blocking(move || {
            let result = active.accept_pcm16(16_000, &audio);
            (active, result)
        })
        .await
        .map_err(task_error)?;
        self.streaming = Some(decoded.0);
        self.audio_seq = self.audio_seq.saturating_add(1);

        let mut events = Vec::new();
        if let Some(mut event) = decoded.1? {
            event["seq"] = json!(self.audio_seq);
            events.push(event);
        }
        events.extend(boundaries.into_iter().map(|boundary| {
            json!({
                "type":"recording_segment_saved","ok":true,"seq":self.audio_seq,
                "segment_index":boundary.index,"start_ms":boundary.start_ms,
                "end_ms":boundary.end_ms,"bytes":boundary.bytes,
                "duration_ms":boundary.end_ms.saturating_sub(boundary.start_ms)
            })
        }));
        Ok(events)
    }

    pub(super) async fn stop(
        &mut self,
        state: &AppState,
        group_id: &str,
        seq: Value,
    ) -> Result<Vec<Value>, voice_asr::VoiceError> {
        let mut events = Vec::new();
        if let Some(mut active) = self.streaming.take() {
            let event = tokio::task::spawn_blocking(move || active.finish())
                .await
                .map_err(task_error)?;
            if let Some(mut event) = event {
                event["seq"] = seq.clone();
                events.push(event);
            }
        }
        let recordings = finish_recording(self.recording.take()).await?;
        if recordings.is_empty() {
            return Ok(events);
        }
        let diarization_reservation = self
            .persist_artifacts
            .then(|| voice_diarization::try_reserve(state, &self.diarization_model));
        let can_defer_to_speaker_analysis =
            diarization_reservation.as_ref().is_some_and(Result::is_ok);
        let mut final_asr = voice_final_asr::transcribe_pcm16_segments(
            state.home.clone(),
            self.selected_model.clone(),
            self.language.clone(),
            &recordings,
            can_defer_to_speaker_analysis,
        )
        .await;
        final_asr["seq"] = seq.clone();
        events.push(final_asr);

        if self.persist_artifacts {
            let status = match diarization_reservation.expect("persistence reserves diarization") {
                Ok(reservation) => voice_diarization::spawn(
                    voice_diarization::DiarizationJob {
                        state: state.clone(),
                        group_id: group_id.to_owned(),
                        session_id: self.client_session_id.clone(),
                        document_path: self.document_path.clone(),
                        diarization_model: self.diarization_model.clone(),
                        transcript_model: self.selected_model.clone(),
                        language: self.language.clone(),
                    },
                    recordings,
                    reservation,
                ),
                Err(reason) => voice_diarization::SpawnStatus::Skipped(reason),
            };
            events.push(diarization_event(status, seq));
        }
        Ok(events)
    }

    pub(super) async fn finalize_disconnect(self, state: AppState, group_id: String) {
        voice_ws_lifecycle::finalize_disconnect(
            voice_ws_lifecycle::DisconnectContext {
                state,
                group_id,
                client_session_id: self.client_session_id,
                document_path: self.document_path,
                language: self.language,
                final_model_id: self.selected_model,
                diarization_model_id: self.diarization_model,
                persist_artifacts: self.persist_artifacts,
            },
            self.streaming,
            self.recording,
        )
        .await;
    }
}

async fn finish_recording(
    recording: Option<voice_segmented_recording::SegmentedPcmRecording>,
) -> Result<Vec<voice_segmented_recording::RecordingSegment>, voice_asr::VoiceError> {
    let Some(recording) = recording.filter(|recording| !recording.is_empty()) else {
        return Ok(Vec::new());
    };
    recording.finish().await
}

fn diarization_event(status: voice_diarization::SpawnStatus, seq: Value) -> Value {
    match status {
        voice_diarization::SpawnStatus::Started => json!({
            "type":"diarization_status","ok":true,"seq":seq,"status":"separating_speakers"
        }),
        voice_diarization::SpawnStatus::Skipped(reason) => json!({
            "type":"diarization_skipped","ok":true,"seq":seq,"reason":reason
        }),
    }
}

fn task_error(error: tokio::task::JoinError) -> voice_asr::VoiceError {
    voice_asr::VoiceError::new("asr_task_failed", error.to_string())
}

use super::{
    AsrError, config,
    connection::{self, Codec, Socket, Writer},
    transcript::{Event, Transcript},
};
use crate::{
    AppState,
    routes::assistants::{voice_segmented_recording::SegmentedPcmRecording, voice_ws_lifecycle},
};
use futures_util::{StreamExt, stream::SplitStream};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) struct Active {
    pub reader: SplitStream<Socket>,
    pub writer: Writer,
    pub codec: Codec,
    pub transcript: Transcript,
    pub model: String,
    pub session_id: String,
    pub document_path: String,
    pub language: String,
    pub persist: bool,
    pub persisted: BTreeSet<String>,
    pub checkpoint_schedule: super::checkpoint_schedule::CheckpointSchedule,
    pub stopping: bool,
    pub completed: bool,
    pub stop_seq: Value,
    recording: SegmentedPcmRecording,
    pending: Vec<u8>,
    audio_seq: u64,
}

impl Active {
    pub async fn start(
        state: &AppState,
        assistant: &Value,
        command: &Value,
    ) -> Result<Self, AsrError> {
        if command["sample_rate"].as_u64().unwrap_or(16000) != 16000 {
            return Err(AsrError::new(
                "unsupported_sample_rate",
                "External ASR requires mono 16000 Hz PCM16",
            ));
        }
        let provider = super::selected(assistant)?;
        let config = config::load(&state.home, provider)?;
        let language = command["language"].as_str().unwrap_or("auto").to_owned();
        let opened = connection::connect(provider, config, &language).await?;
        let mut active = Self::from_opened(&state.home, command, opened)?;
        active.checkpoint_schedule = super::checkpoint_schedule::CheckpointSchedule::new(assistant);
        Ok(active)
    }

    pub(super) fn from_opened(
        home: &cccc_core::HomeLayout,
        command: &Value,
        opened: connection::Opened,
    ) -> Result<Self, AsrError> {
        let language = command["language"].as_str().unwrap_or("auto").to_owned();
        let recording = SegmentedPcmRecording::create(home)?;
        let (writer, reader) = opened.socket.split();
        let session_id = command["session_id"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(Self {
            reader,
            writer,
            codec: opened.codec,
            model: opened.model,
            language,
            session_id,
            document_path: command["document_path"].as_str().unwrap_or_default().into(),
            persist: voice_ws_lifecycle::persists_secretary_artifacts(command),
            persisted: BTreeSet::new(),
            checkpoint_schedule: super::checkpoint_schedule::CheckpointSchedule::new(&Value::Null),
            transcript: Transcript::default(),
            stopping: false,
            completed: false,
            stop_seq: Value::Null,
            recording,
            pending: Vec::new(),
            audio_seq: 0,
        })
    }

    pub fn ready(&self, seq: Value) -> Value {
        json!({"type":"ready","ok":true,"seq":seq,"sample_rate":16000,"audio_transport":"binary_pcm16",
            "model_id":self.model,"backend":"external_provider_asr","provider":self.codec.provider.id(),
            "server_persists_transcript":true})
    }

    pub async fn audio(&mut self, bytes: &[u8]) -> Result<Vec<Value>, AsrError> {
        if self.stopping {
            return Err(AsrError::new(
                "audio_after_stop",
                "Audio received after recording stop",
            ));
        }
        if bytes.len() > 1_048_576 || bytes.len() % 2 != 0 {
            return Err(AsrError::new(
                "invalid_audio",
                "Send even-length PCM16 frames of at most 1 MiB",
            ));
        }
        let boundaries = self.recording.append(bytes).await?;
        self.audio_seq += 1;
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 6400 {
            let chunk: Vec<_> = self.pending.drain(..6400).collect();
            connection::send(&mut self.writer, self.codec.audio(&chunk)?).await?;
        }
        Ok(boundaries
            .into_iter()
            .map(|b| {
                json!({"type":"recording_segment_saved","ok":true,
            "seq":self.audio_seq,"segment_index":b.index,"start_ms":b.start_ms,"end_ms":b.end_ms,
            "duration_ms":b.end_ms.saturating_sub(b.start_ms),"bytes":b.bytes})
            })
            .collect())
    }

    pub async fn stop(&mut self, seq: Value) -> Result<(), AsrError> {
        if self.stopping {
            return Err(AsrError::new(
                "recording_already_stopped",
                "Recording stop is already in progress",
            ));
        }
        self.stopping = true;
        self.stop_seq = seq;
        if self.recording.is_empty() {
            self.completed = true;
            return Ok(());
        }
        if !self.pending.is_empty() {
            let bytes = std::mem::take(&mut self.pending);
            connection::send(&mut self.writer, self.codec.audio(&bytes)?).await?;
        }
        connection::send(&mut self.writer, self.codec.finish()?).await
    }

    pub fn receive(
        &mut self,
        message: tokio_tungstenite::tungstenite::Message,
    ) -> Result<Vec<Value>, AsrError> {
        let (event, done) = self.codec.parse(message)?;
        let mut events = match event {
            Event::Update { segments } => self.transcript.apply(segments, &self.model)?,
            _ => Vec::new(),
        };
        for event in &mut events {
            event["seq"] = json!(self.audio_seq);
        }
        if done {
            if !self.stopping {
                return Err(AsrError::new(
                    "external_asr_ended",
                    "The ASR provider ended recognition before recording was stopped",
                ));
            }
            self.completed = true;
        }
        Ok(events)
    }
}

pub fn transcribe_file(
    home: &HomeLayout,
    model_id: &str,
    path: &Path,
    mime_type: &str,
    language: &str,
) -> Result<Value, VoiceError> {
    if let Some(result) = mock_transcript() {
        return Ok(result);
    }
    let bytes = validate_audio_file(path)?;
    let mime = normalized_mime(mime_type);
    if matches!(mime.as_str(), "audio/wav" | "audio/wave" | "audio/x-wav")
        || file_starts_with_riff(path)?
    {
        return transcribe_wav_file(home, model_id, path, language, bytes);
    }
    if matches!(
        mime.as_str(),
        "audio/pcm" | "audio/l16" | "audio/raw" | "application/octet-stream"
    ) {
        return transcribe_pcm16_file(home, model_id, path, 16_000, language);
    }
    Err(VoiceError::new(
        "unsupported_audio",
        format!("Rust ASR accepts PCM16 or WAV audio, received {mime_type}"),
    ))
}

pub fn transcribe_pcm16_file(
    home: &HomeLayout,
    model_id: &str,
    path: &Path,
    sample_rate: i32,
    language: &str,
) -> Result<Value, VoiceError> {
    if let Some(result) = mock_transcript() {
        return Ok(result);
    }
    let bytes = validate_audio_file(path)?;
    if bytes % 2 != 0 {
        return Err(VoiceError::new(
            "invalid_audio",
            "PCM16 byte length must be even",
        ));
    }
    let (model, recognizer) = create_offline_recognizer(home, model_id, language)?;
    let stream = recognizer.create_stream();
    let mut reader = BufReader::new(
        std::fs::File::open(path)
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?,
    );
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut remaining = bytes;
    while remaining > 0 {
        let chunk_bytes = remaining.min(buffer.len());
        reader
            .read_exact(&mut buffer[..chunk_bytes])
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
        let samples = pcm16_samples(&buffer[..chunk_bytes]);
        stream.accept_waveform(sample_rate, &samples);
        remaining -= chunk_bytes;
    }
    finish_transcription(recognizer, stream, model, sample_rate, bytes)
}

pub fn transcribe_pcm16_ranges(
    home: &HomeLayout,
    model_id: &str,
    path: &Path,
    sample_rate: i32,
    language: &str,
    ranges_ms: &[(i64, i64)],
) -> Result<Vec<Value>, VoiceError> {
    if ranges_ms.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(result) = mock_transcript() {
        return Ok(ranges_ms.iter().map(|_| result.clone()).collect());
    }
    let bytes = validate_audio_file(path)?;
    if bytes % 2 != 0 {
        return Err(VoiceError::new(
            "invalid_audio",
            "PCM16 byte length must be even",
        ));
    }
    if sample_rate <= 0 {
        return Err(VoiceError::new(
            "unsupported_sample_rate",
            "PCM16 sample rate must be positive",
        ));
    }
    let (model, recognizer) = create_offline_recognizer(home, model_id, language)?;
    let mut reader = BufReader::new(
        std::fs::File::open(path)
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?,
    );
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut results = Vec::with_capacity(ranges_ms.len());
    for &(start_ms, end_ms) in ranges_ms {
        let (start_byte, end_byte) = pcm16_byte_range(bytes, sample_rate, start_ms, end_ms);
        let range_bytes = end_byte.saturating_sub(start_byte);
        if range_bytes == 0 {
            results.push(json!({
                "text":"","bytes":0,"model_id":model.model_id,"sample_rate":sample_rate
            }));
            continue;
        }
        reader
            .seek(SeekFrom::Start(start_byte as u64))
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
        let stream = recognizer.create_stream();
        let mut remaining = range_bytes;
        while remaining > 0 {
            let chunk_bytes = remaining.min(buffer.len());
            reader
                .read_exact(&mut buffer[..chunk_bytes])
                .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
            stream.accept_waveform(sample_rate, &pcm16_samples(&buffer[..chunk_bytes]));
            remaining -= chunk_bytes;
        }
        recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| VoiceError::new("asr_failed", "sherpa-onnx returned no result"))?;
        results.push(json!({
            "text":clean_transcript(&result.text),
            "bytes":range_bytes,
            "model_id":model.model_id,
            "sample_rate":sample_rate
        }));
    }
    Ok(results)
}

fn pcm16_byte_range(
    total_bytes: usize,
    sample_rate: i32,
    start_ms: i64,
    end_ms: i64,
) -> (usize, usize) {
    let rate = i64::from(sample_rate.max(1));
    let start_sample = start_ms.max(0).saturating_mul(rate).saturating_div(1000);
    let end_sample = end_ms
        .max(start_ms.max(0))
        .saturating_mul(rate)
        .saturating_div(1000);
    let start_byte = usize::try_from(start_sample)
        .unwrap_or(usize::MAX)
        .saturating_mul(2)
        .min(total_bytes);
    let end_byte = usize::try_from(end_sample)
        .unwrap_or(usize::MAX)
        .saturating_mul(2)
        .min(total_bytes);
    (start_byte, end_byte.max(start_byte))
}

fn create_offline_recognizer(
    home: &HomeLayout,
    model_id: &str,
    language: &str,
) -> Result<(Model, OfflineRecognizer), VoiceError> {
    let model = offline_model(home, model_id)?;
    let config = offline_recognizer_config(home, &model, language)?;
    let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
        VoiceError::new(
            "asr_runtime_not_ready",
            "failed to initialize sherpa-onnx offline recognizer",
        )
    })?;
    Ok((model, recognizer))
}

fn finish_transcription(
    recognizer: OfflineRecognizer,
    stream: sherpa_onnx::OfflineStream,
    model: Model,
    sample_rate: i32,
    bytes: usize,
) -> Result<Value, VoiceError> {
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or_else(|| VoiceError::new("asr_failed", "sherpa-onnx returned no result"))?;
    Ok(
        json!({"text":clean_transcript(&result.text),"bytes":bytes,"model_id":model.model_id,"sample_rate":sample_rate}),
    )
}

fn transcribe_wav_file(
    home: &HomeLayout,
    model_id: &str,
    path: &Path,
    language: &str,
    bytes: usize,
) -> Result<Value, VoiceError> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| VoiceError::new("invalid_audio", error.to_string()))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(VoiceError::new(
            "unsupported_audio",
            "WAV input must be mono",
        ));
    }
    let sample_rate = spec.sample_rate as i32;
    let (model, recognizer) = create_offline_recognizer(home, model_id, language)?;
    let stream = recognizer.create_stream();
    let mut chunk = Vec::with_capacity(32 * 1024);
    match spec.sample_format {
        hound::SampleFormat::Int => {
            for sample in reader.samples::<i16>() {
                chunk.push(
                    f32::from(
                        sample
                            .map_err(|error| VoiceError::new("invalid_audio", error.to_string()))?,
                    ) / 32768.0,
                );
                accept_full_chunk(&stream, sample_rate, &mut chunk);
            }
        }
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                chunk.push(
                    sample.map_err(|error| VoiceError::new("invalid_audio", error.to_string()))?,
                );
                accept_full_chunk(&stream, sample_rate, &mut chunk);
            }
        }
    }
    if !chunk.is_empty() {
        stream.accept_waveform(sample_rate, &chunk);
    }
    finish_transcription(recognizer, stream, model, sample_rate, bytes)
}

fn accept_full_chunk(stream: &sherpa_onnx::OfflineStream, sample_rate: i32, chunk: &mut Vec<f32>) {
    if chunk.len() == chunk.capacity() {
        stream.accept_waveform(sample_rate, chunk);
        chunk.clear();
    }
}

fn validate_audio_file(path: &Path) -> Result<usize, VoiceError> {
    let bytes = std::fs::metadata(path)
        .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?
        .len();
    if bytes == 0 {
        return Err(VoiceError::new(
            "empty_audio",
            "audio payload cannot be empty",
        ));
    }
    let bytes = usize::try_from(bytes)
        .map_err(|_| VoiceError::new("audio_too_large", "audio payload exceeds 100 MiB"))?;
    if bytes > MAX_AUDIO_BYTES {
        return Err(VoiceError::new(
            "audio_too_large",
            "audio payload exceeds 100 MiB",
        ));
    }
    Ok(bytes)
}

fn normalized_mime(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn file_starts_with_riff(path: &Path) -> Result<bool, VoiceError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
    let mut prefix = [0_u8; 4];
    let read = file
        .read(&mut prefix)
        .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
    Ok(read == prefix.len() && &prefix == b"RIFF")
}

fn mock_transcript() -> Option<Value> {
    std::env::var("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT")
        .ok()
        .map(|text| json!({"text":text,"bytes":0,"model_id":"mock","sample_rate":16000}))
}

pub struct StreamingSession {
    recognizer: OnlineRecognizer,
    stream: OnlineStream,
    pub model_id: String,
    last_text: String,
}

pub fn diarize_pcm16_file(
    home: &HomeLayout,
    model_id: &str,
    path: &Path,
    sample_rate: i32,
) -> Result<Option<Value>, VoiceError> {
    let bytes = validate_audio_file(path)?;
    if bytes % 2 != 0 {
        return Err(VoiceError::new(
            "invalid_audio",
            "PCM16 byte length must be even",
        ));
    }
    let mut reader = BufReader::new(
        std::fs::File::open(path)
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?,
    );
    let mut raw = vec![0_u8; 64 * 1024];
    let mut samples = Vec::with_capacity(bytes / 2);
    let mut remaining = bytes;
    while remaining > 0 {
        let chunk_bytes = remaining.min(raw.len());
        reader
            .read_exact(&mut raw[..chunk_bytes])
            .map_err(|error| VoiceError::new("audio_read_failed", error.to_string()))?;
        samples.extend(pcm16_samples(&raw[..chunk_bytes]));
        remaining -= chunk_bytes;
    }
    diarize_samples(home, model_id, &samples, sample_rate)
}

fn diarize_samples(
    home: &HomeLayout,
    model_id: &str,
    samples: &[f32],
    sample_rate: i32,
) -> Result<Option<Value>, VoiceError> {
    let requested = if model_id.trim().is_empty() {
        DEFAULT_DIARIZATION_MODEL_ID
    } else {
        model_id
    };
    let catalog = catalog(home)?;
    let Some(model) = catalog.get(requested).cloned() else {
        return Ok(None);
    };
    let status = model_status(home, &model);
    if status["status"] != "ready" {
        return Ok(None);
    }
    let item = model.diarization.as_ref().ok_or_else(|| {
        VoiceError::new(
            "voice_model_incompatible",
            "selected model has no diarization configuration",
        )
    })?;
    if item.engine != "offline_speaker_diarization" {
        return Err(VoiceError::new(
            "voice_model_incompatible",
            format!("unsupported diarization engine: {}", item.engine),
        ));
    }
    if sample_rate != item.sample_rate {
        return Err(VoiceError::new(
            "unsupported_sample_rate",
            format!("diarization requires {} Hz PCM16", item.sample_rate),
        ));
    }
    let root = model_dir(home, &model.model_id);
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(
                    root.join(&item.segmentation_model)
                        .to_string_lossy()
                        .into_owned(),
                ),
            },
            num_threads: item.num_threads.max(1),
            debug: false,
            provider: Some(item.provider.clone()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(
                root.join(&item.embedding_model)
                    .to_string_lossy()
                    .into_owned(),
            ),
            num_threads: item.num_threads.max(1),
            debug: false,
            provider: Some(item.provider.clone()),
        },
        clustering: FastClusteringConfig {
            num_clusters: item.num_speakers,
            threshold: item.cluster_threshold,
        },
        min_duration_on: item.min_duration_on,
        min_duration_off: item.min_duration_off,
    };
    let diarizer = OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
        VoiceError::new(
            "diarization_runtime_not_ready",
            "failed to initialize sherpa-onnx diarization",
        )
    })?;
    let result = diarizer.process(samples).ok_or_else(|| {
        VoiceError::new(
            "diarization_failed",
            "sherpa-onnx returned no diarization result",
        )
    })?;
    let segments = result
        .sort_by_start_time()
        .into_iter()
        .map(|segment| {
            json!({
                "start_ms":(segment.start*1000.0).round() as i64,
                "end_ms":(segment.end*1000.0).round() as i64,
                "start":segment.start,
                "end":segment.end,
                "speaker":segment.speaker,
                "speaker_label":format!("Speaker {}",segment.speaker+1)
            })
        })
        .collect::<Vec<_>>();
    Ok(Some(json!({
        "model_id":model.model_id,
        "num_speakers":result.num_speakers(),
        "segments":segments,
        "provisional":false
    })))
}

impl StreamingSession {
    pub fn open(home: &HomeLayout, model_id: &str) -> Result<Self, VoiceError> {
        let model = streaming_model(home, model_id)?;
        let config = online_recognizer_config(home, &model)?;
        let recognizer = OnlineRecognizer::create(&config).ok_or_else(|| {
            VoiceError::new(
                "asr_runtime_not_ready",
                "failed to initialize sherpa-onnx streaming recognizer",
            )
        })?;
        let stream = recognizer.create_stream();
        Ok(Self {
            recognizer,
            stream,
            model_id: model.model_id,
            last_text: String::new(),
        })
    }

    pub fn accept_pcm16(
        &mut self,
        sample_rate: i32,
        bytes: &[u8],
    ) -> Result<Option<Value>, VoiceError> {
        if sample_rate != 16_000 {
            return Err(VoiceError::new(
                "unsupported_sample_rate",
                "streaming ASR requires 16000 Hz PCM16",
            ));
        }
        if bytes.len() % 2 != 0 {
            return Err(VoiceError::new(
                "invalid_audio",
                "PCM16 byte length must be even",
            ));
        }
        let samples = pcm16_samples(bytes);
        self.stream.accept_waveform(sample_rate, &samples);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        self.current_event(false)
    }

    pub fn finish(&mut self) -> Option<Value> {
        self.stream.input_finished();
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        self.current_event(true).ok().flatten()
    }

    fn current_event(&mut self, force_final: bool) -> Result<Option<Value>, VoiceError> {
        let Some(result) = self.recognizer.get_result(&self.stream) else {
            return Ok(None);
        };
        let text = clean_transcript(&result.text);
        if text.is_empty() || (!force_final && text == self.last_text) {
            return Ok(None);
        }
        self.last_text.clone_from(&text);
        let is_final = force_final || result.is_final || self.recognizer.is_endpoint(&self.stream);
        if is_final {
            self.recognizer.reset(&self.stream);
            self.last_text.clear();
        }
        Ok(Some(
            json!({"type":if is_final{"final"}else{"partial"},"ok":true,"text":text,"is_final":is_final,"model_id":self.model_id}),
        ))
    }
}

fn offline_recognizer_config(
    home: &HomeLayout,
    model: &Model,
    language: &str,
) -> Result<OfflineRecognizerConfig, VoiceError> {
    let item = model.offline.as_ref().ok_or_else(|| {
        VoiceError::new(
            "voice_model_incompatible",
            "selected model has no offline ASR configuration",
        )
    })?;
    if item.engine != "sense_voice" {
        return Err(VoiceError::new(
            "voice_model_incompatible",
            format!("unsupported offline ASR engine: {}", item.engine),
        ));
    }
    let root = model_dir(home, &model.model_id);
    let mut config = OfflineRecognizerConfig::default();
    config.feat_config.sample_rate = item.sample_rate;
    config.model_config.num_threads = item.num_threads.max(1);
    config.model_config.provider = Some(item.provider.clone());
    config.model_config.tokens = Some(root.join(&item.tokens).to_string_lossy().into_owned());
    config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
        model: Some(root.join(&item.model).to_string_lossy().into_owned()),
        language: Some(normalize_language(if language.trim().is_empty() {
            &item.language
        } else {
            language
        })),
        use_itn: item.use_itn,
    };
    Ok(config)
}

fn online_recognizer_config(
    home: &HomeLayout,
    model: &Model,
) -> Result<OnlineRecognizerConfig, VoiceError> {
    let item = model.streaming.as_ref().ok_or_else(|| {
        VoiceError::new(
            "voice_model_incompatible",
            "selected model has no streaming ASR configuration",
        )
    })?;
    if item.engine != "paraformer" {
        return Err(VoiceError::new(
            "voice_model_incompatible",
            format!("unsupported streaming ASR engine: {}", item.engine),
        ));
    }
    let root = model_dir(home, &model.model_id);
    let mut config = OnlineRecognizerConfig::default();
    config.feat_config.sample_rate = item.sample_rate;
    config.model_config.num_threads = item.num_threads.max(1);
    config.model_config.provider = Some(item.provider.clone());
    config.model_config.tokens = Some(root.join(&item.tokens).to_string_lossy().into_owned());
    config.model_config.paraformer.encoder =
        Some(root.join(&item.encoder).to_string_lossy().into_owned());
    config.model_config.paraformer.decoder =
        Some(root.join(&item.decoder).to_string_lossy().into_owned());
    if !item.joiner.is_empty() {
        return Err(VoiceError::new(
            "voice_model_incompatible",
            "Paraformer model must not define a joiner",
        ));
    }
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = 2.4;
    config.rule2_min_trailing_silence = 1.2;
    config.rule3_min_utterance_length = 20.0;
    Ok(config)
}

fn offline_model(home: &HomeLayout, requested: &str) -> Result<Model, VoiceError> {
    select_model(home, requested, DEFAULT_OFFLINE_MODEL_ID, |model| {
        model.offline.is_some()
    })
}
fn streaming_model(home: &HomeLayout, requested: &str) -> Result<Model, VoiceError> {
    select_model(home, requested, DEFAULT_STREAMING_MODEL_ID, |model| {
        model.streaming.is_some()
    })
}
fn select_model(
    home: &HomeLayout,
    requested: &str,
    fallback: &str,
    supports: impl Fn(&Model) -> bool,
) -> Result<Model, VoiceError> {
    let catalog = catalog(home)?;
    let candidate = catalog
        .get(requested)
        .filter(|model| supports(model))
        .or_else(|| catalog.get(fallback));
    let model = candidate.cloned().ok_or_else(|| {
        VoiceError::new(
            "voice_model_not_found",
            format!("voice ASR model not found: {requested}"),
        )
    })?;
    let status = model_status(home, &model);
    if status["status"] != "ready" {
        return Err(VoiceError::new(
            "voice_model_not_installed",
            format!("voice model is not installed: {}", model.model_id),
        )
        .detail("model", status));
    }
    Ok(model)
}

pub(super) fn pcm16_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
        .collect()
}

pub fn clean_transcript(text: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("<|") {
        cleaned.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        let Some(end) = tail.find("|>") else {
            cleaned.push_str(&remaining[start..]);
            remaining = "";
            break;
        };
        let tag = &tail[..end];
        if tag.is_empty() || tag.len() > 48 || tag.contains(['<', '>', '|']) {
            cleaned.push_str(&remaining[start..start + end + 4]);
        }
        remaining = &tail[end + 2..];
    }
    cleaned.push_str(remaining);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_language(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "zh-cn" | "zh-hans" | "zh" => "zh",
        "en-us" | "en-gb" | "en" => "en",
        "ja-jp" | "ja" => "ja",
        "ko-kr" | "ko" => "ko",
        "zh-hk" | "zh-yue" | "yue" => "yue",
        _ => "auto",
    }
    .into()
}
use super::{
    DEFAULT_DIARIZATION_MODEL_ID, DEFAULT_OFFLINE_MODEL_ID, DEFAULT_STREAMING_MODEL_ID,
    MAX_AUDIO_BYTES, Model, VoiceError, catalog, model_dir, model_status,
};
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use sherpa_onnx::{
    FastClusteringConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
    OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, SpeakerEmbeddingExtractorConfig,
};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

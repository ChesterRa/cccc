use std::time::Duration;

use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;

use super::{voice_asr, voice_inference, voice_segmented_recording::RecordingSegment};

const INFERENCE_CHUNK_DURATION_MS: u64 = 30_000;
// The caller (WS stop / disconnect handling) stops renewing the recording lease while it
// awaits this transcription, and the lease TTL defaults to 30s (cccc-core
// voice_recording_lease). Waiting longer than the TTL would let another client claim the
// lease while this connection still appends its final transcript, so cap the wait well
// below the TTL and fail fast instead.
const INFERENCE_ACQUIRE_TIMEOUT_MS: u64 = 10_000;

pub(super) fn try_acquire() -> Option<OwnedSemaphorePermit> {
    voice_inference::try_acquire()
}

pub(super) async fn transcribe_file(
    permit: OwnedSemaphorePermit,
    home: HomeLayout,
    model_id: String,
    language: String,
    audio_file: tempfile::NamedTempFile,
    mime_type: String,
) -> Result<Result<Value, voice_asr::VoiceError>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        voice_asr::transcribe_file(&home, &model_id, audio_file.path(), &mime_type, &language)
    })
    .await
}

pub(super) async fn transcribe_pcm16_segments(
    home: HomeLayout,
    model_id: String,
    language: String,
    recordings: &[RecordingSegment],
    can_defer_to_speaker_analysis: bool,
) -> Value {
    if recordings.is_empty() {
        return result_payload(Err(voice_asr::VoiceError::new(
            "empty_audio",
            "audio payload cannot be empty",
        )));
    }
    let recordings = recordings
        .iter()
        .map(|recording| RecordingPath {
            index: recording.index,
            start_ms: recording.start_ms,
            end_ms: recording.end_ms,
            bytes: recording.bytes,
            path: recording.file.path().to_owned(),
        })
        .collect::<Vec<_>>();
    let duration_ms = recordings
        .iter()
        .map(|recording| recording.end_ms.saturating_sub(recording.start_ms))
        .sum::<u64>();
    if should_defer_long_recording(can_defer_to_speaker_analysis, duration_ms) {
        return deferred_result_payload("long_recording");
    }
    let permit = match try_acquire() {
        Some(permit) => permit,
        None if can_defer_to_speaker_analysis => {
            return deferred_result_payload("worker_busy");
        }
        None => match tokio::time::timeout(
            Duration::from_millis(INFERENCE_ACQUIRE_TIMEOUT_MS),
            voice_inference::acquire(),
        )
        .await
        {
            Ok(permit) => permit,
            Err(_) => {
                return result_payload(Err(voice_asr::VoiceError::new(
                    "asr_busy",
                    "native inference worker stayed busy past the recording lease deadline",
                )));
            }
        },
    };
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        transcribe_recordings(&home, &model_id, &language, recordings)
    })
    .await;
    match outcome {
        Ok(results) => segmented_result_payload(results),
        Err(error) => json!({
            "type":"final_asr_text","ok":false,
            "error":{"code":"asr_task_failed","message":error.to_string(),"details":{}}
        }),
    }
}

fn deferred_result_payload(reason: &str) -> Value {
    json!({
        "type":"final_asr_status",
        "ok":true,
        "status":"deferred_to_speaker_analysis",
        "reason":reason
    })
}

fn should_defer_long_recording(can_defer_to_speaker_analysis: bool, duration_ms: u64) -> bool {
    can_defer_to_speaker_analysis && duration_ms > INFERENCE_CHUNK_DURATION_MS
}

struct RecordingPath {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    bytes: usize,
    path: std::path::PathBuf,
}

struct SegmentAsrResult {
    index: usize,
    recording_segment_index: usize,
    start_ms: u64,
    end_ms: u64,
    bytes: usize,
    result: Result<Value, voice_asr::VoiceError>,
}

fn transcribe_recordings(
    home: &HomeLayout,
    model_id: &str,
    language: &str,
    recordings: Vec<RecordingPath>,
) -> Vec<SegmentAsrResult> {
    let mut output = Vec::new();
    let mut index = 1;
    for recording in recordings {
        let ranges = inference_ranges(recording.end_ms.saturating_sub(recording.start_ms));
        match voice_asr::transcribe_pcm16_ranges_partial(
            home,
            model_id,
            &recording.path,
            16_000,
            language,
            &ranges,
        ) {
            Ok(results) => {
                for ((start_ms, end_ms), result) in ranges.into_iter().zip(results) {
                    let bytes = result
                        .as_ref()
                        .ok()
                        .and_then(|value| value["bytes"].as_u64())
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_else(|| pcm16_bytes(end_ms.saturating_sub(start_ms)));
                    output.push(SegmentAsrResult {
                        index,
                        recording_segment_index: recording.index,
                        start_ms: recording.start_ms.saturating_add(start_ms as u64),
                        end_ms: recording.start_ms.saturating_add(end_ms as u64),
                        bytes,
                        result,
                    });
                    index += 1;
                }
            }
            Err(error) => {
                output.push(SegmentAsrResult {
                    index,
                    recording_segment_index: recording.index,
                    start_ms: recording.start_ms,
                    end_ms: recording.end_ms,
                    bytes: recording.bytes,
                    result: Err(error),
                });
                index += 1;
            }
        }
    }
    output
}

fn inference_ranges(duration_ms: u64) -> Vec<(i64, i64)> {
    let mut ranges = Vec::new();
    let mut start_ms = 0;
    while start_ms < duration_ms {
        let end_ms = start_ms
            .saturating_add(INFERENCE_CHUNK_DURATION_MS)
            .min(duration_ms);
        ranges.push((start_ms as i64, end_ms as i64));
        start_ms = end_ms;
    }
    ranges
}

fn pcm16_bytes(duration_ms: i64) -> usize {
    usize::try_from(duration_ms.max(0))
        .unwrap_or(usize::MAX)
        .saturating_mul(16_000 * 2)
        / 1_000
}

fn segmented_result_payload(results: Vec<SegmentAsrResult>) -> Value {
    let mut text = Vec::new();
    let mut segments = Vec::with_capacity(results.len());
    let mut first_error = None;
    let mut failed_segment_count = 0;
    let mut model_id = Value::Null;
    let mut sample_rate = Value::Null;
    for item in results {
        match item.result {
            Ok(result) => {
                let segment_text =
                    voice_asr::clean_transcript(result["text"].as_str().unwrap_or(""));
                if !segment_text.is_empty() {
                    text.push(segment_text.clone());
                }
                model_id = result["model_id"].clone();
                sample_rate = result["sample_rate"].clone();
                segments.push(json!({
                    "index":item.index,"recording_segment_index":item.recording_segment_index,
                    "start_ms":item.start_ms,"end_ms":item.end_ms,
                    "bytes":item.bytes,"ok":true,"text":segment_text,
                    "model_id":result["model_id"],"sample_rate":result["sample_rate"]
                }));
            }
            Err(error) => {
                failed_segment_count += 1;
                if first_error.is_none() {
                    first_error = Some((error.code, error.message.clone(), error.details.clone()));
                }
                segments.push(json!({
                    "index":item.index,"recording_segment_index":item.recording_segment_index,
                    "start_ms":item.start_ms,"end_ms":item.end_ms,
                    "bytes":item.bytes,"ok":false,
                    "error":{"code":error.code,"message":error.message,"details":error.details}
                }));
            }
        }
    }
    if text.is_empty() {
        let (code, message, details) = first_error.unwrap_or_else(|| {
            (
                "empty_transcript",
                "final ASR returned no transcript".into(),
                serde_json::Map::new(),
            )
        });
        return json!({
            "type":"final_asr_text","ok":false,"segments":segments,
            "error":{"code":code,"message":message,"details":details}
        });
    }
    let segment_count = segments.len();
    json!({
        "type":"final_asr_text","ok":true,"text":text.join("\n"),
        "model_id":model_id,"sample_rate":sample_rate,"segments":segments,
        "segment_count":segment_count,"partial":failed_segment_count > 0,
        "failed_segment_count":failed_segment_count
    })
}

fn result_payload(result: Result<Value, voice_asr::VoiceError>) -> Value {
    match result {
        Ok(result) => json!({
            "type":"final_asr_text","ok":true,"text":result["text"],
            "model_id":result["model_id"],"sample_rate":result["sample_rate"]
        }),
        Err(error) => json!({
            "type":"final_asr_text","ok":false,
            "error":{"code":error.code,"message":error.message,"details":error.details}
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn final_asr_result_uses_the_websocket_contract() {
        let success = result_payload(Ok(json!({
            "text":"final transcript","model_id":"sense-voice","sample_rate":16000
        })));
        assert_eq!(success["type"], "final_asr_text");
        assert_eq!(success["ok"], true);
        assert_eq!(success["text"], "final transcript");

        let failure = result_payload(Err(voice_asr::VoiceError {
            code: "voice_model_not_installed",
            message: "missing final model".into(),
            details: Map::new(),
        }));
        assert_eq!(failure["type"], "final_asr_text");
        assert_eq!(failure["ok"], false);
        assert_eq!(failure["error"]["code"], "voice_model_not_installed");
    }

    #[test]
    fn busy_final_asr_defers_without_reporting_transcript_failure() {
        let payload = deferred_result_payload("worker_busy");
        assert_eq!(payload["type"], "final_asr_status");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["status"], "deferred_to_speaker_analysis");
        assert_eq!(payload["reason"], "worker_busy");
    }

    #[test]
    fn long_recording_defer_status_is_distinct_from_worker_contention() {
        let payload = deferred_result_payload("long_recording");
        assert_eq!(payload["status"], "deferred_to_speaker_analysis");
        assert_eq!(payload["reason"], "long_recording");
        assert!(should_defer_long_recording(true, 30_001));
        assert!(!should_defer_long_recording(true, 30_000));
        assert!(!should_defer_long_recording(false, 30_001));
    }

    #[test]
    fn inference_acquire_wait_stays_shorter_than_the_recording_lease_ttl() {
        const RECORDING_LEASE_TTL_MS: u64 = 30_000;
        assert!(
            std::hint::black_box(INFERENCE_ACQUIRE_TIMEOUT_MS)
                < std::hint::black_box(RECORDING_LEASE_TTL_MS)
        );
        let payload = result_payload(Err(voice_asr::VoiceError::new(
            "asr_busy",
            "native inference worker stayed busy past the recording lease deadline",
        )));
        assert_eq!(payload["type"], "final_asr_text");
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["error"]["code"], "asr_busy");
    }

    #[test]
    fn segmented_final_asr_keeps_order_and_partial_success() {
        let payload = segmented_result_payload(vec![
            SegmentAsrResult {
                index: 1,
                recording_segment_index: 1,
                start_ms: 0,
                end_ms: 1_500_000,
                bytes: 48_000_000,
                result: Ok(json!({
                    "text":"第一段","model_id":"sense-voice","sample_rate":16000
                })),
            },
            SegmentAsrResult {
                index: 2,
                recording_segment_index: 2,
                start_ms: 1_500_000,
                end_ms: 1_800_000,
                bytes: 9_600_000,
                result: Err(voice_asr::VoiceError::new("asr_failed", "second failed")),
            },
        ]);

        assert_eq!(payload["ok"], true);
        assert_eq!(payload["text"], "第一段");
        assert_eq!(payload["segment_count"], 2);
        assert_eq!(payload["partial"], true);
        assert_eq!(payload["failed_segment_count"], 1);
        assert_eq!(payload["segments"][1]["error"]["code"], "asr_failed");
    }

    #[test]
    fn long_recordings_are_split_into_short_inference_ranges() {
        assert_eq!(INFERENCE_CHUNK_DURATION_MS, 30_000);
        assert_eq!(
            inference_ranges(75_000),
            vec![(0, 30_000), (30_000, 60_000), (60_000, 75_000)]
        );
        assert!(inference_ranges(0).is_empty());
        assert_eq!(pcm16_bytes(30_000), 960_000);
    }
}

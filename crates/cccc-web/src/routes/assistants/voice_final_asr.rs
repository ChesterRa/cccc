use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;

use super::{voice_asr, voice_inference, voice_segmented_recording::RecordingSegment};

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
) -> Value {
    if recordings.is_empty() {
        return result_payload(Err(voice_asr::VoiceError::new(
            "empty_audio",
            "audio payload cannot be empty",
        )));
    }
    let Some(permit) = try_acquire() else {
        return result_payload(Err(voice_asr::VoiceError::new(
            "asr_busy",
            "final ASR is busy with another recording",
        )));
    };
    let segments = recordings
        .iter()
        .map(|recording| {
            (
                recording.index,
                recording.start_ms,
                recording.end_ms,
                recording.bytes,
                recording.file.path().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        segments
            .into_iter()
            .map(|(index, start_ms, end_ms, bytes, path)| {
                let result =
                    voice_asr::transcribe_pcm16_file(&home, &model_id, &path, 16_000, &language);
                SegmentAsrResult {
                    index,
                    start_ms,
                    end_ms,
                    bytes,
                    result,
                }
            })
            .collect::<Vec<_>>()
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

struct SegmentAsrResult {
    index: usize,
    start_ms: u64,
    end_ms: u64,
    bytes: usize,
    result: Result<Value, voice_asr::VoiceError>,
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
                    "index":item.index,"start_ms":item.start_ms,"end_ms":item.end_ms,
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
                    "index":item.index,"start_ms":item.start_ms,"end_ms":item.end_ms,
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
    fn segmented_final_asr_keeps_order_and_partial_success() {
        let payload = segmented_result_payload(vec![
            SegmentAsrResult {
                index: 1,
                start_ms: 0,
                end_ms: 1_500_000,
                bytes: 48_000_000,
                result: Ok(json!({
                    "text":"第一段","model_id":"sense-voice","sample_rate":16000
                })),
            },
            SegmentAsrResult {
                index: 2,
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
}

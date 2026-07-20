use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;

use super::{voice_asr, voice_inference};

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

pub(super) async fn transcribe_pcm16_file(
    home: HomeLayout,
    model_id: String,
    language: String,
    recording: tempfile::NamedTempFile,
) -> (tempfile::NamedTempFile, Value) {
    let Some(permit) = try_acquire() else {
        return (
            recording,
            result_payload(Err(voice_asr::VoiceError {
                code: "asr_busy",
                message: "final ASR is busy with another recording".into(),
                details: serde_json::Map::new(),
            })),
        );
    };
    let path = recording.path().to_owned();
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        voice_asr::transcribe_pcm16_file(&home, &model_id, &path, 16_000, &language)
    })
    .await;
    let payload = match outcome {
        Ok(result) => result_payload(result),
        Err(error) => json!({
            "type":"final_asr_text","ok":false,
            "error":{"code":"asr_task_failed","message":error.to_string(),"details":{}}
        }),
    };
    (recording, payload)
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
}

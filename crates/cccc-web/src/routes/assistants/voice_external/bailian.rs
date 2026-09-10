//! https://help.aliyun.com/zh/model-studio/fun-asr-realtime-websocket-api
use super::{
    AsrError,
    config::Config,
    transcript::{Event, Segment},
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

pub(super) fn start(config: &Config, task: &str, language: &str) -> Message {
    let mut parameters = json!({"format":"pcm","sample_rate":16000});
    let language = language.split('-').next().unwrap_or_default();
    if !language.is_empty() && !matches!(language, "auto" | "mixed") {
        parameters["language_hints"] = json!([if language == "yue" { "zh" } else { language }]);
    }
    Message::Text(
        json!({"header":{"action":"run-task","task_id":task,"streaming":"duplex"},
        "payload":{"task_group":"audio","task":"asr","function":"recognition",
            "model":config.model,"parameters":parameters,"input":{}}})
        .to_string()
        .into(),
    )
}

pub(super) fn finish(task: &str) -> Message {
    Message::Text(
        json!({"header":{"action":"finish-task","task_id":task,"streaming":"duplex"},
        "payload":{"input":{}}})
        .to_string()
        .into(),
    )
}

pub(super) fn parse(bytes: &[u8], task: &str) -> Result<Event, AsrError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| protocol_error())?;
    if value["header"]["task_id"].as_str() != Some(task) {
        return Err(protocol_error());
    }
    match value["header"]["event"].as_str().unwrap_or_default() {
        "task-started" => Ok(Event::Ready),
        "task-finished" => Ok(Event::Finished),
        "task-failed" => {
            let code = value["header"]["error_code"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let (kind, message) = if code.contains("apikey")
                || code.contains("unauthorized")
                || code.contains("forbidden")
                || code.contains("accessdenied")
            {
                (
                    "external_asr_auth_failed",
                    "Bailian authentication failed; check the API key and region",
                )
            } else if code.contains("quota") || code.contains("throttl") || code.contains("rate") {
                (
                    "external_asr_quota",
                    "Bailian ASR quota or rate limit was reached",
                )
            } else {
                (
                    "external_asr_provider_error",
                    "Bailian rejected the recognition task; check the model and account configuration",
                )
            };
            Err(AsrError::new(kind, message))
        }
        "result-generated" => {
            let sentence = &value["payload"]["output"]["sentence"];
            if sentence["heartbeat"].as_bool().unwrap_or(false) {
                return Ok(Event::Ignore);
            }
            let text = sentence["text"].as_str().ok_or_else(protocol_error)?;
            if text.is_empty() {
                return Ok(Event::Ignore);
            }
            let start_ms = sentence["begin_time"].as_u64().ok_or_else(protocol_error)?;
            let id = sentence["sentence_id"].as_u64().unwrap_or(start_ms);
            Ok(Event::Update {
                segments: vec![Segment {
                    id: format!("b:{id:020}"),
                    text: text.to_owned(),
                    start_ms,
                    end_ms: sentence["end_time"].as_u64().unwrap_or(start_ms),
                    finalized: sentence["sentence_end"].as_bool().unwrap_or(false),
                }],
            })
        }
        _ => Ok(Event::Ignore),
    }
}

fn protocol_error() -> AsrError {
    AsrError::new(
        "external_asr_protocol_error",
        "Invalid Bailian recognition response",
    )
}

//! https://www.volcengine.com/docs/6561/1354869
use super::{
    AsrError,
    transcript::{Event, Segment},
};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use std::io::{Read, Write};
use tokio_tungstenite::tungstenite::Message;

pub(super) const MAX_REPLY_BYTES: usize = 1_048_576;

pub(super) fn start(task: &str) -> Result<Message, AsrError> {
    frame(1, 0, 1, json!({"user":{"uid":task},"audio":{"format":"pcm","codec":"raw","rate":16000,"bits":16,"channel":1},
        "request":{"model_name":"bigmodel","enable_itn":true,"enable_punc":true,"show_utterances":true,
            "result_type":"single","enable_nonstream":true}}).to_string().as_bytes())
}

pub(super) fn audio(bytes: &[u8], last: bool) -> Result<Message, AsrError> {
    frame(2, if last { 2 } else { 0 }, 0, bytes)
}

fn frame(kind: u8, flags: u8, serialization: u8, bytes: &[u8]) -> Result<Message, AsrError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).map_err(|_| protocol_error())?;
    let payload = encoder.finish().map_err(|_| protocol_error())?;
    let size = u32::try_from(payload.len()).map_err(|_| protocol_error())?;
    let mut frame = vec![0x11, (kind << 4) | flags, (serialization << 4) | 1, 0];
    frame.extend(size.to_be_bytes());
    frame.extend(payload);
    Ok(Message::Binary(frame.into()))
}

pub(super) fn parse(bytes: &[u8]) -> Result<(Event, bool), AsrError> {
    if bytes.len() < 8 || bytes[0] >> 4 != 1 {
        return Err(protocol_error());
    }
    let offset = usize::from(bytes[0] & 15) * 4;
    if offset < 4 || offset > bytes.len() {
        return Err(protocol_error());
    }
    let kind = bytes[1] >> 4;
    let flags = bytes[1] & 15;
    if kind == 15 {
        return Err(AsrError::new(
            "external_asr_provider_error",
            "Volcengine rejected the recognition task; check the resource, quota and credentials",
        ));
    }
    if kind != 9 {
        return Err(protocol_error());
    }
    let mut position = offset;
    let mut complete = flags & 2 != 0;
    if flags & 1 != 0 {
        let raw = bytes
            .get(position..position + 4)
            .ok_or_else(protocol_error)?;
        let sequence = i32::from_be_bytes(raw.try_into().map_err(|_| protocol_error())?);
        complete |= sequence < 0;
        position += 4;
    }
    let raw = bytes
        .get(position..position + 4)
        .ok_or_else(protocol_error)?;
    let size = u32::from_be_bytes(raw.try_into().map_err(|_| protocol_error())?) as usize;
    position += 4;
    if size > MAX_REPLY_BYTES || bytes.len() != position + size {
        return Err(protocol_error());
    }
    let payload = match bytes[2] & 15 {
        0 => bytes[position..].to_vec(),
        1 => {
            let mut data = Vec::new();
            GzDecoder::new(&bytes[position..])
                .take((MAX_REPLY_BYTES + 1) as u64)
                .read_to_end(&mut data)
                .map_err(|_| protocol_error())?;
            if data.len() > MAX_REPLY_BYTES {
                return Err(protocol_error());
            }
            data
        }
        _ => return Err(protocol_error()),
    };
    if bytes[2] >> 4 != 1 {
        return Err(protocol_error());
    }
    let value: Value = serde_json::from_slice(&payload).map_err(|_| protocol_error())?;
    if value["code"]
        .as_i64()
        .is_some_and(|c| ![0, 1000, 20000000].contains(&c))
    {
        return Err(AsrError::new(
            "external_asr_provider_error",
            "Volcengine could not recognize this audio",
        ));
    }
    let raw_result = &value["result"];
    let result = raw_result
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(raw_result);
    let Some(text) = result["text"].as_str() else {
        return Ok((Event::Ready, complete));
    };
    let segments = result["utterances"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|part| {
            let start_ms = part["start_time"].as_u64().unwrap_or(0);
            Segment {
                id: format!("v:{start_ms}"),
                text: part["text"].as_str().unwrap_or_default().to_owned(),
                start_ms,
                end_ms: part["end_time"].as_u64().unwrap_or(start_ms),
                finalized: part["definite"].as_bool().unwrap_or(false),
            }
        })
        .collect::<Vec<_>>();
    // Incremental results avoid retransmitting a meeting's full word history
    // on every update. Requested utterance boundaries retain each prior sentence.
    if segments.is_empty() {
        if text.is_empty() {
            return Ok((Event::Ready, complete));
        }
        return Err(protocol_error());
    }
    Ok((Event::Update { segments }, complete))
}

fn protocol_error() -> AsrError {
    AsrError::new(
        "external_asr_protocol_error",
        "Invalid Volcengine recognition response",
    )
}

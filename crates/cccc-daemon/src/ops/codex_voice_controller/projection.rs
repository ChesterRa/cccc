use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;

const MAX_SPEAKABLE_RESULT_BYTES: usize = 32 * 1024;
const CONTEXT_CHUNK_BYTES: usize = 500;

#[derive(Debug, Default)]
pub(super) struct SpeakableProgress {
    buffer: String,
    emitted: String,
}

#[derive(Debug, Default)]
pub(super) struct CallProjection {
    pub(super) progress: SpeakableProgress,
    pub(super) delegation_id: String,
    pub(super) projected: bool,
}

#[derive(Debug, Default)]
pub(super) struct CallState {
    pub(super) projections: HashMap<String, CallProjection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalProjection {
    pub delegation_id: String,
    pub commands: Vec<Value>,
}

impl SpeakableProgress {
    pub(super) fn push(&mut self, delta: &str) -> Result<Vec<String>> {
        if self
            .emitted
            .len()
            .saturating_add(self.buffer.len())
            .saturating_add(delta.len())
            > MAX_SPEAKABLE_RESULT_BYTES
        {
            bail!("Voice Analyst progress exceeds {MAX_SPEAKABLE_RESULT_BYTES} bytes");
        }
        self.buffer.push_str(delta);
        let mut ready = Vec::new();
        loop {
            let boundary = if self.streamed() {
                paragraph_boundary(&self.buffer)
            } else {
                second_sentence_boundary(&self.buffer)
            };
            let Some(boundary) = boundary else {
                break;
            };
            let remainder = self.buffer.split_off(boundary);
            let chunk = std::mem::replace(&mut self.buffer, remainder);
            self.emitted.push_str(&chunk);
            let chunk = chunk.trim();
            if !chunk.is_empty() {
                ready.push(chunk.to_owned());
            }
        }
        Ok(ready)
    }

    pub(super) fn finish(&mut self, fallback: &str) -> Vec<String> {
        // Only an exact prefix proves which part of the authoritative final was
        // already projected. A final-only message or correction may differ from
        // the deltas; replaying that final is safer than silently losing it.
        let emitted = std::mem::take(&mut self.emitted);
        self.buffer.clear();
        let value = fallback.trim();
        let value = value.strip_prefix(emitted.trim()).unwrap_or(value).trim();
        if value.is_empty() {
            Vec::new()
        } else {
            vec![value.to_owned()]
        }
    }

    pub(super) fn streamed(&self) -> bool {
        !self.emitted.is_empty()
    }
}

fn second_sentence_boundary(value: &str) -> Option<usize> {
    let characters = value.char_indices().collect::<Vec<_>>();
    let mut sentence_ends = 0;
    for (index, &(_, character)) in characters.iter().enumerate() {
        let cjk_end = matches!(character, '。' | '！' | '？');
        if !cjk_end && !matches!(character, '.' | '!' | '?') {
            continue;
        }
        let mut next = index + 1;
        while next < characters.len()
            && matches!(
                characters[next].1,
                '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '）' | '】' | '」' | '』'
            )
        {
            next += 1;
        }
        if !cjk_end && next < characters.len() && !characters[next].1.is_whitespace() {
            continue;
        }
        sentence_ends += 1;
        if sentence_ends == 2 {
            return Some(if next < characters.len() {
                characters[next].0
            } else {
                value.len()
            });
        }
    }
    None
}

fn paragraph_boundary(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'\n' {
            continue;
        }
        let mut next = index + 1;
        while next < bytes.len() && matches!(bytes[next], b' ' | b'\t' | b'\r') {
            next += 1;
        }
        if next < bytes.len() && bytes[next] == b'\n' {
            return Some(next + 1);
        }
    }
    None
}

pub(super) fn session_context_commands(value: &str) -> Vec<Value> {
    utf8_chunks(value, CONTEXT_CHUNK_BYTES)
        .into_iter()
        .map(|text| {
            json!({
                "type":"session.context.append",
                "channel":"speakable",
                "content":[{"type":"input_text","text":text}],
            })
        })
        .collect()
}

pub(super) fn delegation_context_commands(delegation_id: &str, value: &str) -> Vec<Value> {
    utf8_chunks(value, CONTEXT_CHUNK_BYTES)
        .into_iter()
        .map(|text| {
            json!({
                "type":"delegation.context.append",
                "delegation_item_id":delegation_id,
                "channel":"speakable",
                "content":[{"type":"input_text","text":text}],
            })
        })
        .collect()
}

pub(super) fn utf8_chunks(value: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if !current.is_empty() && current.len() + character.len_utf8() > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

pub(super) fn validate_final_result(result: &str) -> Result<&str> {
    let result = result.trim();
    if result.is_empty() {
        bail!("Voice Analyst final is empty");
    }
    if result.len() > MAX_SPEAKABLE_RESULT_BYTES {
        bail!("Voice Analyst final exceeds {MAX_SPEAKABLE_RESULT_BYTES} bytes");
    }
    Ok(result)
}

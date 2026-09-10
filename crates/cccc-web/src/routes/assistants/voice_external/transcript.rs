use super::AsrError;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) struct Segment {
    pub id: String,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub finalized: bool,
}

pub(super) enum Event {
    Ready,
    Update { segments: Vec<Segment> },
    Finished,
    Ignore,
}

#[derive(Default)]
pub(super) struct Transcript {
    segments: BTreeMap<String, Segment>,
    emitted: BTreeSet<String>,
    last_partial: String,
    segment_bytes: usize,
}

impl Transcript {
    pub fn apply(&mut self, segments: Vec<Segment>, model: &str) -> Result<Vec<Value>, AsrError> {
        let mut events = Vec::new();
        for segment in segments {
            if !segment.text.trim().is_empty() {
                if segment.finalized && self.emitted.insert(segment.id.clone()) {
                    events.push(
                        json!({"type":"final","ok":true,"text":segment.text,"is_final":true,
                        "start_ms":segment.start_ms,"end_ms":segment.end_ms,"model_id":model,
                        "backend":"external_provider_asr_streaming","segment_id":segment.id}),
                    );
                    self.last_partial.clear();
                } else if !segment.finalized
                    && !self.emitted.contains(&segment.id)
                    && segment.text != self.last_partial
                {
                    self.last_partial.clone_from(&segment.text);
                    events.push(
                        json!({"type":"partial","ok":true,"text":segment.text,"is_final":false,
                        "start_ms":segment.start_ms,"end_ms":segment.end_ms,"model_id":model,
                        "backend":"external_provider_asr_streaming","segment_id":segment.id}),
                    );
                }
            }
            self.segment_bytes += segment.text.len();
            if let Some(old) = self.segments.insert(segment.id.clone(), segment) {
                self.segment_bytes -= old.text.len();
            }
        }
        if self.segments.len() > 50_000 || self.segment_bytes > 4_000_000 {
            return Err(AsrError::new(
                "external_asr_transcript_limit",
                "External ASR transcript limit reached",
            ));
        }
        Ok(events)
    }
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.segments.values()
    }
    pub fn text(&self) -> String {
        let mut segments: Vec<_> = self.segments.values().collect();
        segments.sort_by_key(|s| (s.start_ms, s.end_ms));
        segments
            .iter()
            .filter(|s| !s.text.trim().is_empty())
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn final_event(&self, model: &str, complete: bool, seq: Value) -> Value {
        json!({"type":"final_asr_text","ok":true,"text":self.text(),"model_id":model,
            "backend":"external_provider_asr_final","partial":!complete,"seq":seq,
            "failed_segment_count":usize::from(!complete)})
    }
}

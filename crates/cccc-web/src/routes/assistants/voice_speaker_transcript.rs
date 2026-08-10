use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use super::voice_asr;

const MIN_DIARIZATION_DURATION_MS: i64 = 250;
const DIARIZATION_MERGE_GAP_MS: i64 = 350;
const MIN_TRANSCRIBE_DURATION_MS: i64 = 300;
const MERGE_SAME_SPEAKER_GAP_MS: i64 = 1_800;
const MERGE_MAX_TURN_DURATION_MS: i64 = 30_000;
const DISPLAY_MERGE_GAP_MS: i64 = 2_400;
const DISPLAY_MAX_TURN_DURATION_MS: i64 = 45_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpeakerTurn {
    start_ms: i64,
    end_ms: i64,
    speaker_index: i64,
    speaker_label: String,
}

#[derive(Clone, Debug)]
struct RawSpeakerTurn {
    start_ms: i64,
    end_ms: i64,
    speaker_key: String,
}

pub(super) struct SpeakerTranscriptOutcome {
    pub(super) segments: Vec<Value>,
    pub(super) model_id: String,
}

pub(super) fn normalize_diarization_result(result: &mut Value) {
    let segments = normalize_diarization_segments(&result["segments"]);
    let speaker_count = segments
        .iter()
        .filter_map(|segment| segment["speaker_index"].as_i64())
        .max()
        .map_or(0, |value| value + 1);
    result["segments"] = json!(segments);
    result["num_speakers"] = json!(speaker_count);
}

pub(super) fn build(
    home: &HomeLayout,
    model_id: &str,
    recording_path: &Path,
    language: &str,
    diarization: &Value,
) -> Result<SpeakerTranscriptOutcome, voice_asr::VoiceError> {
    let turns = transcription_turns(&diarization["segments"]);
    let ranges = turns
        .iter()
        .map(|turn| (turn.start_ms, turn.end_ms))
        .collect::<Vec<_>>();
    let transcripts = voice_asr::transcribe_pcm16_ranges(
        home,
        model_id,
        recording_path,
        16_000,
        language,
        &ranges,
    )?;
    let actual_model_id = transcripts
        .iter()
        .find_map(|result| result["model_id"].as_str())
        .unwrap_or(model_id)
        .to_owned();
    let segments = turns
        .iter()
        .zip(transcripts)
        .enumerate()
        .filter_map(|(index, (turn, transcript))| {
            let text = voice_asr::clean_transcript(transcript["text"].as_str().unwrap_or(""));
            (!text.is_empty()).then(|| {
                json!({
                    "segment_id":format!("speaker-final-{}",index + 1),
                    "start_ms":turn.start_ms,
                    "end_ms":turn.end_ms,
                    "speaker_label":turn.speaker_label,
                    "speaker_index":turn.speaker_index,
                    "text":text,
                    "language":language,
                    "is_final":true,
                    "trigger":{
                        "trigger_kind":"speaker_transcript",
                        "capture_mode":"document",
                        "recognition_backend":"assistant_service_local_asr_final",
                        "model_id":transcript["model_id"]
                    }
                })
            })
        })
        .collect::<Vec<_>>();
    Ok(SpeakerTranscriptOutcome {
        segments: merge_display_segments(segments),
        model_id: actual_model_id,
    })
}

fn normalize_diarization_segments(value: &Value) -> Vec<Value> {
    let mut turns = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let start_ms = item["start_ms"].as_i64()?.max(0);
            let end_ms = item["end_ms"].as_i64()?.max(0);
            if end_ms - start_ms < MIN_DIARIZATION_DURATION_MS {
                return None;
            }
            let speaker_key = item
                .get("speaker_index")
                .or_else(|| item.get("speaker"))
                .filter(|value| !value.is_null())
                .map(Value::to_string)
                .or_else(|| {
                    item["speaker_label"]
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                })?;
            Some(RawSpeakerTurn {
                start_ms,
                end_ms,
                speaker_key,
            })
        })
        .collect::<Vec<_>>();
    turns.sort_by_key(|turn| (turn.start_ms, turn.end_ms));
    absorb_short_speaker_clusters(&mut turns);

    let mut speaker_map = HashMap::<String, i64>::new();
    let mut normalized = Vec::<SpeakerTurn>::with_capacity(turns.len());
    for turn in turns {
        let next_index = speaker_map.len() as i64;
        let speaker_index = *speaker_map.entry(turn.speaker_key).or_insert(next_index);
        let item = SpeakerTurn {
            start_ms: turn.start_ms,
            end_ms: turn.end_ms,
            speaker_index,
            speaker_label: format!("Speaker {}", speaker_index + 1),
        };
        if let Some(previous) = normalized.last_mut()
            && previous.speaker_index == item.speaker_index
            && item.start_ms - previous.end_ms <= DIARIZATION_MERGE_GAP_MS
        {
            previous.end_ms = previous.end_ms.max(item.end_ms);
        } else {
            normalized.push(item);
        }
    }
    normalized
        .into_iter()
        .map(|turn| {
            json!({
                "start_ms":turn.start_ms,
                "end_ms":turn.end_ms,
                "speaker_label":turn.speaker_label,
                "speaker_index":turn.speaker_index
            })
        })
        .collect()
}

fn absorb_short_speaker_clusters(turns: &mut [RawSpeakerTurn]) {
    if turns.len() < 3 {
        return;
    }
    let mut totals = BTreeMap::<String, i64>::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for turn in turns.iter() {
        *totals.entry(turn.speaker_key.clone()).or_default() += turn.end_ms - turn.start_ms;
        *counts.entry(turn.speaker_key.clone()).or_default() += 1;
    }
    if totals.len() <= 1 {
        return;
    }
    let span_ms = turns
        .last()
        .map(|turn| turn.end_ms)
        .unwrap_or(0)
        .saturating_sub(turns.first().map(|turn| turn.start_ms).unwrap_or(0));
    let (min_cluster_duration_ms, min_single_turn_ms) = if span_ms <= 12_000 {
        (
            (span_ms * 20 / 100).max(1_500),
            (span_ms * 12 / 100).max(900),
        )
    } else {
        (1_500, 900)
    };
    let short_keys = totals
        .iter()
        .filter(|(key, total)| {
            **total < min_cluster_duration_ms
                || (counts.get(*key).copied().unwrap_or(0) == 1 && **total < min_single_turn_ms)
        })
        .map(|(key, _)| key.clone())
        .collect::<BTreeSet<_>>();
    let stable_keys = totals
        .keys()
        .filter(|key| !short_keys.contains(*key))
        .cloned()
        .collect::<BTreeSet<_>>();
    if short_keys.is_empty() || stable_keys.is_empty() {
        return;
    }

    let snapshot = turns.to_vec();
    for (index, turn) in turns.iter_mut().enumerate() {
        if !short_keys.contains(&turn.speaker_key) {
            continue;
        }
        let mut best: Option<(&str, i64, i64)> = None;
        for (other_index, other) in snapshot.iter().enumerate() {
            if other_index == index || !stable_keys.contains(&other.speaker_key) {
                continue;
            }
            let gap = if other.end_ms <= turn.start_ms {
                turn.start_ms - other.end_ms
            } else if turn.end_ms <= other.start_ms {
                other.start_ms - turn.end_ms
            } else {
                0
            };
            let total = totals.get(&other.speaker_key).copied().unwrap_or(0);
            if best.is_none_or(|(_, best_gap, best_total)| {
                gap < best_gap || (gap == best_gap && total > best_total)
            }) {
                best = Some((&other.speaker_key, gap, total));
            }
        }
        if let Some((speaker_key, _, _)) = best {
            turn.speaker_key = speaker_key.to_owned();
        }
    }
}

fn transcription_turns(value: &Value) -> Vec<SpeakerTurn> {
    let mut turns = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let start_ms = item["start_ms"].as_i64()?.max(0);
            let end_ms = item["end_ms"].as_i64()?.max(0);
            let speaker_label = item["speaker_label"].as_str()?.trim().to_owned();
            if end_ms <= start_ms || speaker_label.is_empty() {
                return None;
            }
            Some(SpeakerTurn {
                start_ms,
                end_ms,
                speaker_index: item["speaker_index"].as_i64().unwrap_or(-1),
                speaker_label,
            })
        })
        .collect::<Vec<_>>();
    turns.sort_by_key(|turn| (turn.start_ms, turn.end_ms));

    let mut merged = Vec::<SpeakerTurn>::new();
    for turn in turns {
        if let Some(previous) = merged.last_mut() {
            let overlaps = turn.start_ms < previous.end_ms;
            let same_speaker = previous.speaker_label == turn.speaker_label
                && previous.speaker_index == turn.speaker_index;
            let gap_ms = turn.start_ms - previous.end_ms;
            let merged_duration_ms = turn.end_ms - previous.start_ms;
            if !overlaps
                && same_speaker
                && (0..=MERGE_SAME_SPEAKER_GAP_MS).contains(&gap_ms)
                && merged_duration_ms <= MERGE_MAX_TURN_DURATION_MS
            {
                previous.end_ms = turn.end_ms;
                continue;
            }
        }
        merged.push(turn);
    }
    merged
        .into_iter()
        .filter(|turn| turn.end_ms - turn.start_ms >= MIN_TRANSCRIBE_DURATION_MS)
        .collect()
}

fn merge_display_segments(segments: Vec<Value>) -> Vec<Value> {
    let mut merged = Vec::<Value>::new();
    for mut segment in segments {
        let text = voice_asr::clean_transcript(segment["text"].as_str().unwrap_or(""));
        if text.is_empty() {
            continue;
        }
        segment["text"] = json!(text);
        if let Some(previous) = merged.last_mut() {
            let start_ms = segment["start_ms"].as_i64().unwrap_or(0);
            let previous_end_ms = previous["end_ms"].as_i64().unwrap_or(0);
            let overlaps = start_ms < previous_end_ms;
            let same_speaker = previous["speaker_label"] == segment["speaker_label"]
                && previous["speaker_index"] == segment["speaker_index"];
            let gap_ms = start_ms - previous_end_ms;
            let merged_duration_ms = segment["end_ms"].as_i64().unwrap_or(0)
                - previous["start_ms"].as_i64().unwrap_or(0);
            if !overlaps
                && same_speaker
                && (0..=DISPLAY_MERGE_GAP_MS).contains(&gap_ms)
                && merged_duration_ms <= DISPLAY_MAX_TURN_DURATION_MS
            {
                previous["end_ms"] = segment["end_ms"].clone();
                previous["text"] = json!(join_transcript_text(
                    previous["text"].as_str().unwrap_or(""),
                    segment["text"].as_str().unwrap_or("")
                ));
                continue;
            }
        }
        merged.push(segment);
    }
    merged
}

fn join_transcript_text(previous: &str, next: &str) -> String {
    let left = voice_asr::clean_transcript(previous);
    let right = voice_asr::clean_transcript(next);
    if left.is_empty() {
        return right;
    }
    if right.is_empty() || left.ends_with(&right) {
        return left;
    }
    if right.starts_with(&left) {
        return right;
    }
    let cjk_boundary =
        left.chars().next_back().is_some_and(is_cjk) && right.chars().next().is_some_and(is_cjk);
    let punctuation_boundary = left
        .chars()
        .next_back()
        .is_some_and(|ch| ".!?;:，。！？；：、".contains(ch));
    if cjk_boundary || punctuation_boundary {
        format!("{left}{right}")
    } else {
        format!("{left} {right}")
    }
}

fn is_cjk(ch: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&ch) || ('\u{3400}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diarization_relabels_merges_and_drops_tiny_turns() {
        let normalized = normalize_diarization_segments(&json!([
            {"speaker_label":"Speaker 6","speaker_index":5,"start_ms":0,"end_ms":100},
            {"speaker_label":"Speaker 6","speaker_index":5,"start_ms":1000,"end_ms":1800},
            {"speaker_label":"Speaker 6","speaker_index":5,"start_ms":1900,"end_ms":2500},
            {"speaker_label":"Speaker 2","speaker_index":1,"start_ms":3000,"end_ms":3700}
        ]));

        assert_eq!(
            json!(normalized),
            json!([
                {"speaker_label":"Speaker 1","speaker_index":0,"start_ms":1000,"end_ms":2500},
                {"speaker_label":"Speaker 2","speaker_index":1,"start_ms":3000,"end_ms":3700}
            ])
        );
    }

    #[test]
    fn short_spurious_speaker_cluster_is_absorbed() {
        let normalized = normalize_diarization_segments(&json!([
            {"speaker":0,"start_ms":0,"end_ms":1800},
            {"speaker":8,"start_ms":1800,"end_ms":2300},
            {"speaker":0,"start_ms":2300,"end_ms":4200},
            {"speaker":1,"start_ms":4300,"end_ms":6200}
        ]));

        assert_eq!(
            json!(normalized),
            json!([
                {"speaker_label":"Speaker 1","speaker_index":0,"start_ms":0,"end_ms":4200},
                {"speaker_label":"Speaker 2","speaker_index":1,"start_ms":4300,"end_ms":6200}
            ])
        );
    }

    #[test]
    fn adjacent_same_speaker_turns_merge_before_asr_but_overlaps_do_not() {
        let turns = transcription_turns(&json!([
            {"speaker_label":"Speaker 1","speaker_index":0,"start_ms":0,"end_ms":1200},
            {"speaker_label":"Speaker 1","speaker_index":0,"start_ms":1800,"end_ms":3600},
            {"speaker_label":"Speaker 1","speaker_index":0,"start_ms":3000,"end_ms":4200}
        ]));

        assert_eq!(turns.len(), 2);
        assert_eq!((turns[0].start_ms, turns[0].end_ms), (0, 3600));
        assert_eq!((turns[1].start_ms, turns[1].end_ms), (3000, 4200));
    }

    #[test]
    fn speaker_windows_keep_the_complete_timeline_beyond_48_turns() {
        let segments = (0..60)
            .map(|index| {
                json!({
                    "speaker_label":format!("Speaker {}",index % 2 + 1),
                    "speaker_index":index % 2,
                    "start_ms":index * 1_000,
                    "end_ms":index * 1_000 + 800
                })
            })
            .collect::<Vec<_>>();

        let turns = transcription_turns(&json!(segments));

        assert_eq!(turns.len(), 60);
        assert_eq!(turns.last().map(|turn| turn.end_ms), Some(59_800));
    }

    #[test]
    fn display_merge_cleans_tags_and_joins_cjk_text() {
        let merged = merge_display_segments(vec![
            json!({"speaker_label":"Speaker 1","speaker_index":0,"start_ms":0,"end_ms":1000,"text":"<|zh|>一个"}),
            json!({"speaker_label":"Speaker 1","speaker_index":0,"start_ms":2400,"end_ms":3400,"text":"<|HAPPY|>模型"}),
            json!({"speaker_label":"Speaker 2","speaker_index":1,"start_ms":3600,"end_ms":4500,"text":"next"}),
        ]);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["text"], "一个模型");
        assert_eq!(merged[0]["end_ms"], 3400);
    }

    #[test]
    #[ignore = "requires installed native models and an explicit PCM16 fixture"]
    fn installed_models_process_real_pcm16() {
        let home_path = std::env::var("CCCC_TEST_HOME").expect("CCCC_TEST_HOME");
        let pcm16_path = std::env::var("CCCC_TEST_PCM16_PATH").expect("CCCC_TEST_PCM16_PATH");
        let home = HomeLayout::from_path(home_path).expect("home");
        let mut diarization = voice_asr::diarize_pcm16_file(
            &home,
            voice_asr::DEFAULT_DIARIZATION_MODEL_ID,
            Path::new(&pcm16_path),
            16_000,
        )
        .expect("diarization")
        .expect("installed diarization model");
        normalize_diarization_result(&mut diarization);
        let raw_segments = diarization["segments"]
            .as_array()
            .expect("diarization segments");
        assert!(!raw_segments.is_empty());

        let transcript = build(
            &home,
            voice_asr::DEFAULT_OFFLINE_MODEL_ID,
            Path::new(&pcm16_path),
            "mixed",
            &diarization,
        )
        .expect("speaker-window ASR");
        assert!(!transcript.segments.is_empty());
        eprintln!(
            "native diarization: {} ranges, {} speakers, {} transcript rows",
            raw_segments.len(),
            diarization["num_speakers"].as_i64().unwrap_or(0),
            transcript.segments.len()
        );
    }
}

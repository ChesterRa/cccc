use cccc_core::HomeLayout;
use serde_json::{Value, json};

use super::{voice_asr, voice_segmented_recording::RecordingSegment, voice_speaker_transcript};

pub(super) fn analyze(
    home: &HomeLayout,
    diarization_model: &str,
    transcript_model: &str,
    language: &str,
    recordings: &[RecordingSegment],
) -> Result<Option<Value>, voice_asr::VoiceError> {
    let mut combined_ranges = Vec::new();
    let mut combined_transcript = Vec::new();
    let mut speaker_offset = 0_i64;
    let mut actual_diarization_model = String::new();
    let mut actual_transcript_model = String::new();

    for recording in recordings {
        let Some(mut result) =
            voice_asr::diarize_pcm16_file(home, diarization_model, recording.file.path(), 16_000)?
        else {
            continue;
        };
        voice_speaker_transcript::normalize_diarization_result(&mut result);
        let transcript = voice_speaker_transcript::build(
            home,
            transcript_model,
            recording.file.path(),
            language,
            &result,
        )?;
        actual_diarization_model = result["model_id"]
            .as_str()
            .unwrap_or(diarization_model)
            .to_owned();
        actual_transcript_model = transcript.model_id;
        let local_speakers = result["num_speakers"].as_i64().unwrap_or(0).max(0);
        append_with_timeline_offset(
            &mut combined_ranges,
            result["segments"].as_array().cloned().unwrap_or_default(),
            recording,
            speaker_offset,
            false,
        );
        append_with_timeline_offset(
            &mut combined_transcript,
            transcript.segments,
            recording,
            speaker_offset,
            true,
        );
        speaker_offset += local_speakers;
    }

    if combined_ranges.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({
        "model_id":actual_diarization_model,
        "num_speakers":speaker_offset,
        "segments":combined_ranges,
        "provisional":false,
        "recording_segment_count":recordings.len(),
        "speaker_identity_scope":if recordings.len() > 1 {"recording_segment"} else {"session"},
        "speaker_transcript_segments":combined_transcript,
        "speaker_transcript_model_id":actual_transcript_model
    })))
}

fn append_with_timeline_offset(
    target: &mut Vec<Value>,
    segments: Vec<Value>,
    recording: &RecordingSegment,
    speaker_offset: i64,
    rewrite_segment_id: bool,
) {
    for (position, mut segment) in segments.into_iter().enumerate() {
        let start_ms = segment["start_ms"].as_i64().unwrap_or(0).max(0);
        let end_ms = segment["end_ms"].as_i64().unwrap_or(start_ms).max(start_ms);
        let local_speaker = segment["speaker_index"].as_i64().unwrap_or(0).max(0);
        let speaker_index = speaker_offset + local_speaker;
        segment["start_ms"] = json!(recording.start_ms.saturating_add(start_ms as u64));
        segment["end_ms"] = json!(recording.start_ms.saturating_add(end_ms as u64));
        segment["speaker_index"] = json!(speaker_index);
        segment["speaker_label"] = json!(format!("Speaker {}", speaker_index + 1));
        segment["recording_segment_index"] = json!(recording.index);
        if rewrite_segment_id {
            segment["segment_id"] = json!(format!(
                "speaker-final-{}-{}",
                recording.index,
                position + 1
            ));
        }
        target.push(segment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_merge_offsets_time_and_keeps_speaker_names_honest() {
        let temp = tempfile::NamedTempFile::new().expect("recording");
        let recording = RecordingSegment {
            file: temp,
            index: 2,
            start_ms: 1_500_000,
            end_ms: 1_800_000,
            bytes: 9_600_000,
        };
        let mut combined = vec![json!({
            "start_ms":0,"end_ms":1000,"speaker_index":0,"speaker_label":"Speaker 1"
        })];

        append_with_timeline_offset(
            &mut combined,
            vec![json!({
                "segment_id":"speaker-final-1","start_ms":250,"end_ms":1250,
                "speaker_index":0,"speaker_label":"Speaker 1","text":"next"
            })],
            &recording,
            2,
            true,
        );

        assert_eq!(combined[1]["start_ms"], 1_500_250);
        assert_eq!(combined[1]["end_ms"], 1_501_250);
        assert_eq!(combined[1]["speaker_index"], 2);
        assert_eq!(combined[1]["speaker_label"], "Speaker 3");
        assert_eq!(combined[1]["segment_id"], "speaker-final-2-1");
        assert_eq!(combined[1]["recording_segment_index"], 2);
    }
}

use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::dispatch::{OpError, bool_arg, string_arg};

pub(super) struct RevisionMetadata {
    pub(super) stage: String,
    pub(super) revision_only: bool,
    pub(super) supersedes: Vec<String>,
    pub(super) source_model_id: String,
    supersede_live: bool,
}

pub(super) struct SegmentData<'a> {
    pub(super) group_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) segment_id: &'a str,
    pub(super) text: &'a str,
    pub(super) language: &'a str,
    pub(super) document_path: &'a str,
    pub(super) now: &'a str,
}

pub(super) fn resolve(
    request: &DaemonRequest,
    segment_id: &str,
) -> Result<RevisionMetadata, OpError> {
    let stage = string_arg(request, "transcript_stage")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| inferred_stage(request));
    if !matches!(stage.as_str(), "live" | "final") {
        return Err(OpError::new(
            "invalid_args",
            "transcript_stage must be live or final",
        ));
    }
    let revision_only = bool_arg(request, "revision_only", false);
    let is_final = bool_arg(request, "is_final", true);
    if revision_only && (stage != "final" || !is_final) {
        return Err(OpError::new(
            "invalid_args",
            "revision_only requires a stable final transcript segment",
        ));
    }
    let supersede_live = match string_arg(request, "supersede_stage").as_deref() {
        None | Some("") => false,
        Some("live") => true,
        Some(_) => {
            return Err(OpError::new(
                "invalid_args",
                "supersede_stage must be live when provided",
            ));
        }
    };
    if supersede_live && (stage != "final" || !is_final) {
        return Err(OpError::new(
            "invalid_args",
            "supersede_stage=live requires a stable final transcript segment",
        ));
    }
    let mut supersedes = request
        .args
        .get("supersedes_segment_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| safe_id(value) && *value != segment_id)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    supersedes.retain(|value| seen.insert(value.clone()));
    Ok(RevisionMetadata {
        stage,
        revision_only,
        supersedes,
        source_model_id: string_arg(request, "source_model_id").unwrap_or_default(),
        supersede_live,
    })
}

pub(super) fn prepare_for_commit(
    segment: &mut Value,
    current_segments: &[Value],
    revision: &RevisionMetadata,
) {
    if !revision.supersede_live {
        return;
    }
    let segment_id = segment["segment_id"].as_str().unwrap_or_default();
    let mut supersedes = revision.supersedes.clone();
    supersedes.extend(current_segments.iter().filter_map(|current| {
        (segment_stage(current) == "live")
            .then(|| current["segment_id"].as_str().unwrap_or_default().trim())
            .filter(|value| safe_id(value) && *value != segment_id)
            .map(str::to_owned)
    }));
    let mut seen = HashSet::new();
    supersedes.retain(|value| seen.insert(value.clone()));
    segment["supersedes_segment_ids"] = json!(supersedes);
}

pub(super) fn build_segment(
    request: &DaemonRequest,
    data: SegmentData<'_>,
    revision: &RevisionMetadata,
) -> Value {
    json!({
        "schema":1,"segment_id":data.segment_id,"session_id":data.session_id,"group_id":data.group_id,
        "assistant_id":"voice_secretary","text":data.text,"language":data.language,
        "is_final":crate::dispatch::bool_arg(request,"is_final",true),
        "transcript_stage":revision.stage,
        "revision_only":revision.revision_only,
        "supersede_stage":if revision.supersede_live {"live"} else {""},
        "supersedes_segment_ids":revision.supersedes,
        "source_model_id":revision.source_model_id,
        "start_ms":request.args.get("start_ms"),"end_ms":request.args.get("end_ms"),
        "speaker_label":string_arg(request,"speaker_label").unwrap_or_default(),
        "document_path":data.document_path,"trigger":request.args.get("trigger").cloned().unwrap_or_else(||json!({})),
        "by":string_arg(request,"by").unwrap_or_else(||"user".into()),"created_at":data.now,"updated_at":data.now
    })
}

pub(super) fn projected_transcript(segments: &[Value]) -> String {
    projected_segments(segments)
        .filter(|item| item["is_final"].as_bool().unwrap_or(true))
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn contains(segments: &[Value], segment_id: &str) -> bool {
    projected_segments(segments).any(|item| item["segment_id"] == segment_id)
}

fn projected_segments(segments: &[Value]) -> impl Iterator<Item = &Value> {
    let superseded = segments
        .iter()
        .filter(|item| stable_segment(item))
        .flat_map(|item| {
            let session_id = item["session_id"].as_str().unwrap_or_default();
            item["supersedes_segment_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(move |segment_id| (session_id, segment_id))
        })
        .collect::<HashSet<_>>();
    let supersedes_live = segments
        .iter()
        .filter(|item| {
            stable_segment(item)
                && segment_stage(item) == "final"
                && item["supersede_stage"].as_str() == Some("live")
        })
        .filter_map(|item| item["session_id"].as_str())
        .collect::<HashSet<_>>();
    segments.iter().filter(move |item| {
        let session_id = item["session_id"].as_str().unwrap_or_default();
        stable_segment(item)
            && (!supersedes_live.contains(session_id) || segment_stage(item) != "live")
            && item["segment_id"]
                .as_str()
                .is_none_or(|segment_id| !superseded.contains(&(session_id, segment_id)))
    })
}

fn stable_segment(segment: &Value) -> bool {
    segment["is_final"].as_bool().unwrap_or(true)
}

fn inferred_stage(request: &DaemonRequest) -> String {
    let backend = request
        .args
        .get("trigger")
        .and_then(|trigger| trigger.get("recognition_backend"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if backend == "assistant_service_local_asr_final" {
        "final"
    } else {
        "live"
    }
    .into()
}

fn segment_stage(segment: &Value) -> &str {
    segment["transcript_stage"].as_str().unwrap_or_else(|| {
        if segment["trigger"]["recognition_backend"] == "assistant_service_local_asr_final" {
            "final"
        } else {
            "live"
        }
    })
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    fn request(args: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: "assistant_voice_transcript_append".into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        }
    }

    #[test]
    fn revision_only_requires_a_stable_final_segment() {
        let request = request(json!({
            "transcript_stage": "final",
            "revision_only": true,
            "supersede_stage": "live",
            "is_final": false
        }));

        assert!(resolve(&request, "final-asr").is_err());
    }

    #[test]
    fn superseding_live_requires_a_stable_final_segment() {
        let request = request(json!({
            "transcript_stage": "final",
            "revision_only": false,
            "supersede_stage": "live",
            "is_final": false
        }));

        assert!(resolve(&request, "final-asr").is_err());
    }

    #[test]
    fn unknown_supersede_stage_is_rejected() {
        let request = request(json!({
            "transcript_stage": "final",
            "supersede_stage": "lvie",
            "is_final": true
        }));

        assert!(resolve(&request, "final-asr").is_err());
    }

    #[test]
    fn raw_segment_retains_revision_only_metadata() {
        let request = request(json!({
            "transcript_stage": "final",
            "revision_only": true,
            "supersede_stage": "live",
            "is_final": true
        }));
        let revision = resolve(&request, "final-asr").expect("valid final revision");
        let segment = build_segment(
            &request,
            SegmentData {
                group_id: "group",
                session_id: "session",
                segment_id: "final-asr",
                text: "final transcript",
                language: "en",
                document_path: "meeting.md",
                now: "2026-09-03T00:00:00Z",
            },
            &revision,
        );

        assert_eq!(segment["revision_only"], true);
    }

    #[test]
    fn unstable_legacy_revision_cannot_hide_live_projection() {
        let segments = json!([
            {
                "session_id": "session",
                "segment_id": "live",
                "transcript_stage": "live",
                "is_final": true,
                "text": "live transcript"
            },
            {
                "session_id": "session",
                "segment_id": "partial-final",
                "transcript_stage": "final",
                "supersede_stage": "live",
                "supersedes_segment_ids": ["live"],
                "is_final": false,
                "text": "partial final"
            }
        ]);

        assert_eq!(
            projected_transcript(segments.as_array().expect("segments")),
            "live transcript"
        );
    }
}

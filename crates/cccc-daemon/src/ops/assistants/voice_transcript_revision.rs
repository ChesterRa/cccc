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
    if revision_only && stage != "final" {
        return Err(OpError::new(
            "invalid_args",
            "revision_only is valid only for final transcript revisions",
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
        supersede_live: string_arg(request, "supersede_stage").as_deref() == Some("live"),
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
            segment_stage(item) == "final" && item["supersede_stage"].as_str() == Some("live")
        })
        .filter_map(|item| item["session_id"].as_str())
        .collect::<HashSet<_>>();
    segments.iter().filter(move |item| {
        let session_id = item["session_id"].as_str().unwrap_or_default();
        (!supersedes_live.contains(session_id) || segment_stage(item) != "live")
            && item["segment_id"]
                .as_str()
                .is_none_or(|segment_id| !superseded.contains(&(session_id, segment_id)))
    })
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

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{voice_input, voice_input_dedupe};
use crate::dispatch::{OpError, string_arg};

pub(super) struct CandidateContext<'a> {
    pub(super) home: &'a HomeLayout,
    pub(super) request: &'a DaemonRequest,
    pub(super) group_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) segment_id: &'a str,
    pub(super) text: &'a str,
    pub(super) language: &'a str,
    pub(super) document_path: &'a str,
    pub(super) now: &'a str,
    pub(super) segment: &'a Value,
    pub(super) is_final: bool,
    pub(super) auto_document: bool,
    pub(super) segment_superseded: bool,
    pub(super) policy: voice_input_dedupe::Policy,
}

pub(super) fn resolve(context: CandidateContext<'_>) -> Result<(Option<Value>, bool), OpError> {
    let existing = voice_input::find_input(
        context.home,
        context.group_id,
        context.session_id,
        context.segment_id,
    )
    .map_err(OpError::io)?;
    if existing.is_some() {
        return Ok((existing, false));
    }
    let input_kind =
        string_arg(context.request, "input_kind").unwrap_or_else(|| "asr_transcript".into());
    if context.segment_superseded
        || !context.is_final
        || context.text.is_empty()
        || (input_kind == "asr_transcript" && !context.auto_document)
    {
        return Ok((None, false));
    }
    voice_input::append_input(
        context.home,
        context.group_id,
        json!({
            "schema":1,"input_id":format!("vin_{}",Uuid::new_v4().simple()),
            "kind":input_kind,"group_id":context.group_id,"assistant_id":"voice_secretary",
            "text":context.text,"language":context.language,"document_path":context.document_path,
            "session_id":context.session_id,"segment_id":context.segment_id,
            "by":context.segment["by"],"trigger":context.segment["trigger"],
            "request_id":string_arg(context.request,"request_id").unwrap_or_default(),
            "operation":string_arg(context.request,"operation").unwrap_or_default(),
            "composer_snapshot_hash":string_arg(context.request,"composer_snapshot_hash").unwrap_or_default(),
            "metadata":context.request.args.get("metadata").cloned().unwrap_or_else(||json!({})),
            "created_at":context.now,"updated_at":context.now
        }),
        context.policy,
    )
    .map_err(OpError::io)
}

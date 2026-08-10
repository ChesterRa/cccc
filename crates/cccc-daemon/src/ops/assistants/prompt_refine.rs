use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, assistant_state, ledger};
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use super::{voice_input, voice_semantic_input, voice_settings};
use crate::dispatch::{
    OpError, OpResult, bool_arg, first_non_blank_arg, object, required_arg, string_arg,
};

const ACTOR_ID: &str = "voice-secretary";
const ASSISTANT_PRINCIPAL: &str = "assistant:voice_secretary";
const DEFAULT_OPERATION: &str = "append_to_composer_end";
const MAX_RECORDS: usize = 30;

pub fn input(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let kind = string_arg(request, "kind")
        .or_else(|| string_arg(request, "input_kind"))
        .unwrap_or_default();
    if kind != "prompt_refine" {
        return Err(OpError::new(
            "invalid_voice_input_kind",
            "kind must be prompt_refine",
        ));
    }
    let voice_transcript =
        first_non_blank_arg(request, &["voice_transcript", "text"]).unwrap_or_default();
    let composer_text = string_arg(request, "composer_text")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if voice_transcript.is_empty() && composer_text.is_empty() {
        return Err(OpError::new(
            "empty_prompt_refine_input",
            "voice_transcript or composer_text is required",
        ));
    }

    let request_id = clean_request_id(string_arg(request, "request_id"));
    let input_append_id = voice_semantic_input::requested_input_append_id(request)
        .unwrap_or_else(|| format!("voice-prompt-input-{}", Uuid::new_v4().simple()));
    let operation = string_arg(request, "operation")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OPERATION.into());
    let snapshot_hash = string_arg(request, "composer_snapshot_hash").unwrap_or_default();
    let composer_context = request
        .args
        .get("composer_context")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let now = utc_now();

    let prompt_request = update(home, &group_id, |state| {
        let requests = object_mut(state, "voice_prompt_requests");
        let existing = requests
            .get(&request_id)
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut input_append_ids = existing["input_append_ids"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if input_append_ids
            .iter()
            .any(|value| value.as_str() == Some(&input_append_id))
        {
            return Ok(existing);
        }
        input_append_ids.push(json!(input_append_id));
        if input_append_ids.len() > MAX_RECORDS {
            input_append_ids.drain(..input_append_ids.len() - MAX_RECORDS);
        }
        let mut transcripts = existing["voice_transcripts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !voice_transcript.is_empty()
            && transcripts
                .last()
                .and_then(Value::as_str)
                .is_none_or(|value| value != voice_transcript)
        {
            transcripts.push(json!(voice_transcript));
        }
        if transcripts.len() > 12 {
            transcripts.drain(..transcripts.len() - 12);
        }
        let record = json!({
            "schema": 1,
            "group_id": group_id,
            "assistant_id": "voice_secretary",
            "request_id": request_id,
            "operation": operation,
            "composer_text": composer_text,
            "composer_context": composer_context,
            "composer_snapshot_hash": snapshot_hash,
            "voice_transcripts": transcripts,
            "input_append_ids": input_append_ids,
            "last_input_append_id": input_append_id,
            "created_at": existing["created_at"].as_str().unwrap_or(&now),
            "updated_at": now,
        });
        requests.insert(request_id.clone(), record.clone());
        trim_records(requests);

        let drafts = object_mut(state, "voice_prompt_drafts");
        if let Some(draft) = drafts
            .get_mut(&request_id)
            .filter(|draft| draft["status"] == "pending")
        {
            draft["status"] = json!("stale");
            draft["updated_at"] = json!(now);
        }
        if state
            .get("prompt_draft")
            .is_some_and(|draft| draft["request_id"] == request_id)
        {
            state.insert("prompt_draft".into(), Value::Null);
        }
        set_runtime(
            state,
            "working",
            json!({
                "status": "prompt_refine_requested",
                "last_input_kind": "prompt_refine",
                "last_prompt_request_id": request_id,
                "active_request_id": request_id,
                "active_request_kind": "prompt",
                "active_request_status": "pending",
                "last_input_at": now,
            }),
        );
        Ok(record)
    })?;

    let merged_transcript = prompt_request["voice_transcripts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = render_input(
        &request_id,
        &operation,
        &composer_text,
        &merged_transcript,
        &composer_context,
    );
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("request_id".into(), json!(request_id));
    forwarded
        .args
        .insert("input_append_id".into(), json!(input_append_id));
    forwarded.args.insert("operation".into(), json!(operation));
    forwarded
        .args
        .insert("composer_snapshot_hash".into(), json!(snapshot_hash));
    forwarded.args.insert(
        "metadata".into(),
        json!({
            "target_kind": "composer",
            "request_id": request_id,
            "operation": operation,
            "composer_snapshot_hash": snapshot_hash,
            "prompt_request_append_count": prompt_request["voice_transcripts"].as_array().map_or(0, Vec::len),
        }),
    );
    let mut result = voice_input::named(home, &forwarded, "prompt_refine", text)?;
    result.insert("request_id".into(), json!(request_id));
    result.insert("prompt_request".into(), prompt_request);
    Ok(result)
}

pub fn submit(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| ACTOR_ID.into());
    if !matches!(by.as_str(), ACTOR_ID | ASSISTANT_PRINCIPAL) {
        return Err(OpError::new(
            "assistant_voice_prompt_draft_forbidden",
            "prompt drafts can only be submitted by voice-secretary",
        ));
    }
    let request_id = required_arg(request, "request_id")?;
    let no_op = bool_arg(request, "no_op", false);
    let draft_text = string_arg(request, "draft_text")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if draft_text.is_empty() && !no_op {
        return Err(OpError::new(
            "empty_voice_prompt_draft",
            "draft_text is required",
        ));
    }

    let current = load(home, &group_id)?;
    let prompt_request = current["voice_prompt_requests"]
        .get(&request_id)
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            OpError::new(
                "prompt_request_not_found",
                format!("prompt request not found: {request_id}"),
            )
        })?;
    let operation = string_arg(request, "operation")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| prompt_request["operation"].as_str().map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_OPERATION.into());
    let snapshot_hash = string_arg(request, "composer_snapshot_hash")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            prompt_request["composer_snapshot_hash"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let summary = string_arg(request, "summary").unwrap_or_default();
    let now = utc_now();
    let status = if no_op { "no_change" } else { "pending" };
    let draft_preview = draft_text.chars().take(240).collect::<String>();
    let record = update(home, &group_id, |state| {
        let drafts = object_mut(state, "voice_prompt_drafts");
        let created_at = drafts
            .get(&request_id)
            .and_then(|value| value["created_at"].as_str())
            .unwrap_or(&now)
            .to_owned();
        let record = json!({
            "schema": 1,
            "group_id": group_id,
            "assistant_id": "voice_secretary",
            "request_id": request_id,
            "status": status,
            "operation": operation,
            "draft_text": if no_op { "" } else { &draft_text },
            "draft_preview": if no_op { "" } else { &draft_preview },
            "summary": summary,
            "composer_snapshot_hash": snapshot_hash,
            "created_at": created_at,
            "updated_at": now,
            "by": ASSISTANT_PRINCIPAL,
        });
        drafts.insert(request_id.clone(), record.clone());
        trim_records(drafts);
        state.insert(
            "prompt_draft".into(),
            if no_op { Value::Null } else { record.clone() },
        );
        set_runtime(
            state,
            if no_op { "idle" } else { "waiting" },
            json!({
                "status": if no_op { "prompt_draft_no_change" } else { "prompt_draft_ready" },
                "last_prompt_request_id": request_id,
                "last_prompt_draft_at": now,
            }),
        );
        Ok(record)
    })?;
    let event = append_event(
        home,
        &group_id,
        request,
        &record,
        if no_op { "no_op" } else { "submit" },
    )?;
    object(json!({
        "group_id": group_id,
        "assistant": voice_settings::effective_assistant(&load(home, &group_id)?),
        "prompt_draft": record,
        "event": event,
    }))
}

pub fn ack(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let request_id = required_arg(request, "request_id")?;
    let status = required_arg(request, "status")?;
    if !matches!(status.as_str(), "applied" | "dismissed" | "stale") {
        return Err(OpError::new(
            "invalid_prompt_draft_status",
            "status must be applied, dismissed, or stale",
        ));
    }
    let current = load(home, &group_id)?;
    if current["voice_prompt_drafts"]
        .get(&request_id)
        .filter(|value| value.is_object())
        .is_none()
    {
        return Err(OpError::new(
            "prompt_draft_not_found",
            format!("prompt draft not found: {request_id}"),
        ));
    }
    let record = update(home, &group_id, |state| {
        let drafts = object_mut(state, "voice_prompt_drafts");
        let draft = drafts
            .get_mut(&request_id)
            .expect("prompt draft checked before update");
        draft["status"] = json!(status);
        draft["updated_at"] = json!(utc_now());
        let record = draft.clone();
        if state
            .get("prompt_draft")
            .is_some_and(|draft| draft["request_id"] == request_id)
        {
            state.insert("prompt_draft".into(), Value::Null);
        }
        set_runtime(
            state,
            "idle",
            json!({
                "status": format!("prompt_draft_{status}"),
                "last_prompt_request_id": request_id,
                "last_prompt_draft_ack_at": utc_now(),
            }),
        );
        Ok(record)
    })?;
    let event = append_event(home, &group_id, request, &record, "ack")?;
    object(json!({
        "group_id": group_id,
        "assistant": voice_settings::effective_assistant(&load(home, &group_id)?),
        "prompt_draft": record,
        "event": event,
    }))
}

fn render_input(
    request_id: &str,
    operation: &str,
    composer_text: &str,
    voice_transcript: &str,
    composer_context: &Value,
) -> String {
    let draft_rule = if matches!(operation, "replace" | "replace_with_refined_prompt") {
        "draft_text must contain the complete replacement prompt."
    } else {
        "draft_text must contain only the text to add."
    };
    let mut sections = vec![
        "Target: composer".into(),
        "Request kind: prompt_refine".into(),
        format!("Request id: {request_id}"),
        format!("Operation: {operation}"),
    ];
    if !composer_text.is_empty() {
        sections.push(format!("Current composer:\n{composer_text}"));
    }
    if !voice_transcript.is_empty() {
        sections.push(format!("Voice instruction:\n{voice_transcript}"));
    }
    if composer_context
        .as_object()
        .is_some_and(|value| !value.is_empty())
    {
        sections.push(format!("Recent context:\n{composer_context}"));
    }
    sections.push(format!(
        "Required output:\nUse MCP tool cccc_voice_secretary_composer(action=\"submit_prompt_draft\", request_id=\"{request_id}\", draft_text=\"...\").\n{draft_rule}"
    ));
    sections.join("\n\n")
}

fn clean_request_id(value: Option<String>) -> String {
    let raw = value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("voice-prompt-{}", Uuid::new_v4().simple()));
    let clean = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['.', ':', '-'])
        .chars()
        .take(128)
        .collect::<String>();
    if clean.is_empty() {
        format!("voice-prompt-{}", Uuid::new_v4().simple())
    } else {
        clean
    }
}

fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    assistant_state::load(home, group_id).map_err(OpError::io)
}

fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, OpError> {
    assistant_state::update(home, group_id, change).map_err(OpError::io)
}

fn object_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let value = state.entry(key).or_insert_with(|| json!({}));
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object initialized")
}

fn trim_records(records: &mut Map<String, Value>) {
    if records.len() <= MAX_RECORDS {
        return;
    }
    let mut keys = records
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value["updated_at"]
                    .as_str()
                    .or_else(|| value["created_at"].as_str())
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    keys.sort_by(|left, right| left.1.cmp(&right.1));
    for (key, _) in keys.into_iter().take(records.len() - MAX_RECORDS) {
        records.remove(&key);
    }
}

fn set_runtime(state: &mut Map<String, Value>, lifecycle: &str, health: Value) {
    let assistant = state
        .entry("assistant")
        .or_insert_with(|| voice_settings::effective_assistant(&json!({})));
    assistant["lifecycle"] = json!(lifecycle);
    assistant["health"] = health;
    assistant["updated_at"] = json!(utc_now());
    let mirror = assistant.clone();
    state.insert("voice_secretary".into(), mirror);
}

fn append_event(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
    record: &Value,
    action: &str,
) -> Result<Event, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let mut event = Event::new("assistant.voice.prompt_draft", group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| ASSISTANT_PRINCIPAL.into());
    event.data = json!({
        "assistant_id": "voice_secretary",
        "request_id": record["request_id"],
        "action": action,
        "status": record["status"],
        "draft_preview": record["draft_preview"],
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    ledger::append(&store.ledger_path(group_id).map_err(OpError::io)?, &event)
        .map_err(OpError::io)?;
    Ok(event)
}

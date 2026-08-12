use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, assistant_state, ledger};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;

use super::{voice_document_state, voice_input, voice_semantic_input, voice_settings};
use crate::dispatch::{
    OpError, OpResult, bool_arg, first_non_blank_arg, object, required_arg, string_arg,
};

const MAX_ASK_REQUESTS: usize = 30;
const ACTOR_ID: &str = "voice-secretary";
const ASSISTANT_PRINCIPAL: &str = "assistant:voice_secretary";

pub(super) fn input(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let instruction = clean_text(
        first_non_blank_arg(request, &["instruction", "text"])
            .unwrap_or_default()
            .as_str(),
        8_000,
    );
    let source_text = clean_text(
        string_arg(request, "source_text")
            .unwrap_or_default()
            .as_str(),
        32_000,
    );
    if instruction.is_empty() && source_text.is_empty() {
        return Err(OpError::new(
            "empty_voice_instruction",
            "instruction or source_text is required",
        ));
    }

    let request_id = request_id_or_new(string_arg(request, "request_id"));
    let input_append_id = voice_semantic_input::requested_input_append_id(request)
        .unwrap_or_else(|| request_id.clone());
    let document_path = string_arg(request, "document_path")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !document_path.is_empty() {
        voice_input::validate_document_path(&document_path)?;
    }

    let state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let document = if document_path.is_empty() {
        None
    } else {
        let document = state["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["document_path"] == document_path)
            .cloned()
            .ok_or_else(|| {
                OpError::new(
                    "voice_document_not_found",
                    format!("voice document not found: {document_path}"),
                )
            })?;
        if document["status"].as_str().unwrap_or("active") != "active" {
            return Err(OpError::new(
                "voice_document_archived",
                "voice secretary document is archived",
            ));
        }
        Some(document)
    };
    let target_kind = if document.is_some() {
        "document"
    } else {
        "secretary"
    };

    let mut trigger = request
        .args
        .get("trigger")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let language = string_arg(request, "language")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| value_string(&trigger, "language"))
        .unwrap_or_default();
    let intent_hint = value_string(&trigger, "intent_hint").unwrap_or_else(|| {
        if target_kind == "document" {
            "document_instruction".into()
        } else {
            "secretary_task".into()
        }
    });
    set_default(&mut trigger, "trigger_kind", "voice_instruction");
    set_default(
        &mut trigger,
        "mode",
        if target_kind == "document" {
            "meeting"
        } else {
            "voice_instruction"
        },
    );
    trigger.insert("target_kind".into(), json!(target_kind));
    trigger.insert("intent_hint".into(), json!(intent_hint));
    trigger.insert("language".into(), json!(language));
    trigger
        .entry("instruction_policy")
        .or_insert_with(instruction_policy);

    let text = render_input(&instruction, &source_text);
    let mut metadata = request
        .args
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    metadata.insert("target_kind".into(), json!(target_kind));
    metadata.insert("request_id".into(), json!(request_id));

    let now = utc_now();
    let pending = json!({
        "schema":1,
        "request_id":request_id,
        "status":"pending",
        "request_text":text,
        "request_preview":clean_text(&text, 240),
        "reply_text":"",
        "document_path":document_path,
        "artifact_paths":[],
        "source_summary":"",
        "checked_at":"",
        "source_urls":[],
        "target_kind":target_kind,
        "intent_hint":intent_hint,
        "language":language,
        "input_append_id":input_append_id,
        "input_appended_at":now,
        "created_at":now,
        "updated_at":now
    });

    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("request_id".into(), json!(request_id));
    forwarded
        .args
        .insert("input_append_id".into(), json!(input_append_id));
    forwarded
        .args
        .insert("document_path".into(), json!(document_path));
    forwarded.args.insert("language".into(), json!(language));
    forwarded
        .args
        .insert("trigger".into(), Value::Object(trigger));
    forwarded
        .args
        .insert("metadata".into(), Value::Object(metadata));

    let pending_for_state = pending.clone();
    let mut result = voice_semantic_input::append_with_state(
        home,
        &forwarded,
        "voice_instruction",
        text,
        move |root| ensure_pending(root, pending_for_state),
    )?;
    let current = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let ask_request = find_request(&current, &request_id).unwrap_or(pending);
    result.insert("request_id".into(), json!(request_id));
    result.insert("ask_request".into(), ask_request);
    if let Some(document) = document {
        result.insert("document".into(), document);
    }
    Ok(result)
}

pub(super) fn feedback(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| ACTOR_ID.into());
    if !matches!(by.as_str(), ACTOR_ID | ASSISTANT_PRINCIPAL) {
        return Err(OpError::new(
            "assistant_voice_instruction_feedback_forbidden",
            "voice instruction feedback can only be submitted by voice-secretary",
        ));
    }
    let raw_request_id = required_arg(request, "request_id")?;
    let request_id = normalize_request_id(&raw_request_id)
        .ok_or_else(|| OpError::new("invalid_voice_ask_request_id", "invalid request_id"))?;
    let status = required_arg(request, "status")?.to_ascii_lowercase();
    if !matches!(
        status.as_str(),
        "working" | "done" | "needs_user" | "failed"
    ) {
        return Err(OpError::new(
            "invalid_voice_ask_status",
            "status must be working, done, needs_user, or failed",
        ));
    }
    let now = utc_now();
    let reply_text = clean_text(
        first_non_blank_arg(request, &["reply_text", "result_text", "message"])
            .unwrap_or_default()
            .as_str(),
        4_000,
    );
    let document_path = string_arg(request, "document_path")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !document_path.is_empty() {
        voice_input::validate_document_path(&document_path)?;
    }
    let source_summary = clean_text(
        string_arg(request, "source_summary")
            .unwrap_or_default()
            .as_str(),
        1_200,
    );
    let checked_at = clean_text(
        string_arg(request, "checked_at")
            .unwrap_or_default()
            .as_str(),
        120,
    );
    let source_urls = string_list(request.args.get("source_urls"), 12, 1_024, true);
    let mut artifact_paths = string_list(request.args.get("artifact_paths"), 12, 512, false);
    if !document_path.is_empty() && !artifact_paths.contains(&document_path) {
        artifact_paths.push(document_path.clone());
    }

    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let (ask_request, assistant) = assistant_state::update(home, &group_id, |root| {
        let asks = ask_requests(root);
        let index = asks
            .iter()
            .position(|item| item["request_id"] == request_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "request not found"))?;
        let mut item = asks.remove(index);
        item["status"] = json!(status);
        if request.args.contains_key("reply_text")
            || request.args.contains_key("result_text")
            || request.args.contains_key("message")
        {
            item["reply_text"] = json!(reply_text);
        }
        if !document_path.is_empty() {
            item["document_path"] = json!(document_path);
        }
        if request.args.contains_key("artifact_paths") || !document_path.is_empty() {
            item["artifact_paths"] = json!(artifact_paths);
        }
        if request.args.contains_key("source_summary") {
            item["source_summary"] = json!(source_summary);
        }
        if request.args.contains_key("checked_at") {
            item["checked_at"] = json!(checked_at);
        }
        if request.args.contains_key("source_urls") {
            item["source_urls"] = json!(source_urls);
        }
        if !reply_text.is_empty() || matches!(status.as_str(), "needs_user" | "failed") {
            if let Some(item) = item.as_object_mut() {
                item.remove("cleared_at");
            }
        }
        if item["first_feedback_at"]
            .as_str()
            .is_none_or(|value| value.is_empty())
        {
            item["first_feedback_at"] = json!(now);
        }
        item["last_feedback_at"] = json!(now);
        item["updated_at"] = json!(now);
        asks.insert(0, item.clone());
        let assistant = update_runtime_after_feedback(root, &request_id, &status, &now);
        Ok((item, assistant))
    })
    .map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            OpError::new(
                "voice_ask_request_not_found",
                format!("voice ask request not found: {request_id}"),
            )
        } else {
            OpError::io(error)
        }
    })?;

    let event = append_feedback_event(&store, &group_id, &request_id, &status, &ask_request)?;
    object(json!({
        "group_id":group_id,
        "assistant":assistant,
        "ask_request":ask_request,
        "request":ask_request,
        "event":event
    }))
}

pub(super) fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let keep_active = bool_arg(request, "keep_active", false);
    let now = utc_now();
    let (ask_requests, assistant, cleared_count) = update(home, &group_id, |root| {
        let asks = ask_requests(root);
        let mut cleared_count = 0;
        for item in asks.iter_mut() {
            if keep_active && is_active_status(item["status"].as_str().unwrap_or("")) {
                continue;
            }
            if item["cleared_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                continue;
            }
            item["cleared_at"] = json!(now);
            item["updated_at"] = json!(now);
            cleared_count += 1;
        }
        let visible = asks
            .iter()
            .filter(|item| {
                item["cleared_at"]
                    .as_str()
                    .is_none_or(|value| value.is_empty())
            })
            .cloned()
            .collect::<Vec<_>>();
        let assistant = update_runtime_after_clear(root, &visible);
        Ok((visible, assistant, cleared_count))
    })?;
    let kept_count = ask_requests.len();
    object(json!({
        "group_id":group_id,
        "assistant":assistant,
        "ask_requests":ask_requests,
        "latest_ask_request":ask_requests.first().cloned(),
        "cleared_count":cleared_count,
        "removed_count":cleared_count,
        "kept_count":kept_count
    }))
}

fn ensure_pending(root: &mut Map<String, Value>, pending: Value) -> io::Result<()> {
    let request_id = pending["request_id"].as_str().unwrap_or_default();
    let asks = ask_requests(root);
    if asks.iter().any(|item| item["request_id"] == request_id) {
        return Ok(());
    }
    asks.insert(0, pending.clone());
    asks.truncate(MAX_ASK_REQUESTS);
    set_runtime(
        root,
        "working",
        json!({
            "status":"instruction_requested",
            "last_input_kind":"voice_instruction",
            "last_ask_request_id":request_id,
            "active_request_id":request_id,
            "active_request_kind":if pending["target_kind"] == "document" { "document" } else { "ask" },
            "active_request_status":"pending",
            "last_document_path":pending["document_path"],
            "last_input_at":pending["input_appended_at"]
        }),
    );
    Ok(())
}

fn update_runtime_after_feedback(
    root: &mut Map<String, Value>,
    request_id: &str,
    status: &str,
    now: &str,
) -> Value {
    if status == "working" {
        let target_kind = ask_requests(root)
            .iter()
            .find(|item| item["request_id"] == request_id)
            .and_then(|item| item["target_kind"].as_str())
            .unwrap_or("secretary")
            .to_owned();
        return set_runtime(
            root,
            "working",
            json!({
                "status":"ask_working",
                "last_ask_request_id":request_id,
                "active_request_id":request_id,
                "active_request_kind":if target_kind == "document" { "document" } else { "ask" },
                "active_request_status":"working",
                "last_ask_feedback_at":now
            }),
        );
    }
    let next_active = ask_requests(root)
        .iter()
        .find(|item| {
            item["request_id"] != request_id
                && is_active_status(item["status"].as_str().unwrap_or(""))
                && item["cleared_at"]
                    .as_str()
                    .is_none_or(|value| value.is_empty())
        })
        .cloned();
    if let Some(active) = next_active {
        let active_id = active["request_id"].as_str().unwrap_or_default();
        let active_status = active["status"].as_str().unwrap_or("pending");
        set_runtime(
            root,
            "working",
            json!({
                "status":format!("ask_{active_status}"),
                "last_ask_request_id":active_id,
                "active_request_id":active_id,
                "active_request_kind":if active["target_kind"] == "document" { "document" } else { "ask" },
                "active_request_status":active_status,
                "last_ask_feedback_at":now
            }),
        )
    } else {
        set_runtime(
            root,
            if status == "working" {
                "working"
            } else if status == "needs_user" {
                "waiting"
            } else {
                "idle"
            },
            json!({
                "status":format!("ask_{status}"),
                "last_ask_request_id":request_id,
                "active_request_id":if status == "working" { request_id } else { "" },
                "active_request_kind":if status == "working" { "ask" } else { "" },
                "active_request_status":status,
                "last_ask_feedback_at":now
            }),
        )
    }
}

fn update_runtime_after_clear(root: &mut Map<String, Value>, asks: &[Value]) -> Value {
    if let Some(active) = asks
        .iter()
        .find(|item| is_active_status(item["status"].as_str().unwrap_or("")))
    {
        let request_id = active["request_id"].as_str().unwrap_or_default();
        let status = active["status"].as_str().unwrap_or("pending");
        set_runtime(
            root,
            "working",
            json!({
                "status":format!("ask_{status}"),
                "last_ask_request_id":request_id,
                "active_request_id":request_id,
                "active_request_kind":"ask",
                "active_request_status":status
            }),
        )
    } else {
        set_runtime(root, "idle", json!({"status":"ask_history_cleared"}))
    }
}

fn set_runtime(root: &mut Map<String, Value>, lifecycle: &str, health: Value) -> Value {
    let assistant = root
        .entry("assistant")
        .or_insert_with(|| voice_settings::effective_assistant(&json!({})));
    assistant["lifecycle"] = json!(lifecycle);
    assistant["health"] = health;
    assistant["updated_at"] = json!(utc_now());
    let assistant = assistant.clone();
    root.insert("voice_secretary".into(), assistant.clone());
    assistant
}

fn append_feedback_event(
    store: &GroupStore,
    group_id: &str,
    request_id: &str,
    status: &str,
    ask_request: &Value,
) -> Result<Event, OpError> {
    let data = json!({
        "assistant_id":"voice_secretary",
        "request_id":request_id,
        "source_request_id":request_id,
        "action":"report",
        "status":status,
        "document_path":ask_request["document_path"],
        "artifact_paths":ask_request["artifact_paths"],
        "source_summary":ask_request["source_summary"],
        "checked_at":ask_request["checked_at"],
        "source_urls":ask_request["source_urls"],
        "request_preview":ask_request["request_preview"],
        "reply_text":ask_request["reply_text"]
    });
    let event_id = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({"kind":"assistant.voice.request","data":data}))
                .map_err(OpError::invalid)?
        )
    );
    let path = store.ledger_path(group_id).map_err(OpError::io)?;
    if let Some(existing) = ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .find(|event| event.id == event_id)
    {
        return Ok(existing);
    }
    let mut event = Event::new("assistant.voice.request", group_id);
    event.id = event_id;
    event.by = ASSISTANT_PRINCIPAL.into();
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(&path, &event).map_err(OpError::io)?;
    Ok(event)
}

fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, OpError> {
    assistant_state::update(home, group_id, change).map_err(OpError::io)
}

fn ask_requests(root: &mut Map<String, Value>) -> &mut Vec<Value> {
    let value = root.entry("ask_requests").or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("ask_requests initialized")
}

fn find_request(state: &Value, request_id: &str) -> Option<Value> {
    state["ask_requests"]
        .as_array()?
        .iter()
        .find(|item| item["request_id"] == request_id)
        .cloned()
}

fn render_input(instruction: &str, source_text: &str) -> String {
    match (instruction.is_empty(), source_text.is_empty()) {
        (false, true) => format!("Task:\n{instruction}"),
        (false, false) => format!(
            "Task:\n{instruction}\n\nContext (not task):\nAdditional source:\n{source_text}"
        ),
        (true, false) => format!(
            "Task:\nHandle the provided voice input as a secretary Ask request.\n\nInputs:\n{source_text}"
        ),
        (true, true) => String::new(),
    }
}

fn instruction_policy() -> Value {
    json!({
        "default":"classify_each_job_before_writing",
        "memo":"synthesize_into_working_document",
        "document_instruction":"modify_create_or_archive_voice_documents_when_clear",
        "secretary_task":"handle_safe_secretary_scope_work_yourself_when_transcript_backlog_is_clear",
        "peer_task":"handoff_only_when_work_belongs_to_foreman_or_a_concrete_peer",
        "mixed":"split_memo_secretary_work_and_peer_handoffs",
        "document_updates":"safe_to_apply_when_instruction_or_memo_is_clear",
        "new_document":"create_only_when_separate_deliverable_is_clear",
        "handoff":"use_voice_secretary_request_for_explicit_user_requested_peer_or_foreman_work",
        "request_notify":"use_voice_secretary_request_only_for_explicit_handoff_to_foreman_or_one_actor",
        "queue_priority":"while_transcript_jobs_are_pending_prioritize_intake_then_process_secretary_queue",
        "unclear":"record_as_context_or_open_question_do_not_notify_peers"
    })
}

fn set_default(target: &mut Map<String, Value>, key: &str, value: &str) {
    if target
        .get(key)
        .and_then(Value::as_str)
        .is_none_or(|current| current.trim().is_empty())
    {
        target.insert(key.into(), json!(value));
    }
}

fn value_string(value: &Map<String, Value>, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn request_id_or_new(value: Option<String>) -> String {
    value
        .as_deref()
        .and_then(normalize_request_id)
        .unwrap_or_else(|| format!("voice-ask-{}", Uuid::new_v4().simple()))
}

fn normalize_request_id(value: &str) -> Option<String> {
    let clean = value
        .trim()
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
    (!clean.is_empty()).then_some(clean)
}

fn clean_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn string_list(
    value: Option<&Value>,
    limit: usize,
    max_chars: usize,
    urls_only: bool,
) -> Vec<String> {
    let mut values = value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !urls_only || value.starts_with("http://") || value.starts_with("https://"))
        .map(|value| value.chars().take(max_chars).collect::<String>())
        .collect::<Vec<_>>();
    values.dedup();
    values.truncate(limit);
    values
}

fn is_active_status(status: &str) -> bool {
    matches!(status, "pending" | "working")
}

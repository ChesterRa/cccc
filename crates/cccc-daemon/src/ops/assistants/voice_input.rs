use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{GroupStore, HomeLayout, assistant_state};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use super::{voice_document_state, voice_input_delivery, voice_semantic_input};
use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

const ACTOR_ID: &str = "voice-secretary";
const INPUT_STATE_SCHEMA: u64 = 1;

pub fn append(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let session_id = safe_id(&required_arg(request, "session_id")?)?;
    let segment_id = safe_id(
        &string_arg(request, "segment_id")
            .unwrap_or_else(|| format!("seg-{}", Uuid::new_v4().simple())),
    )?;
    let text = string_arg(request, "text")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let flush = bool_arg(request, "flush", false);
    if text.is_empty() && !flush {
        return Err(OpError::new(
            "empty_transcript_segment",
            "text cannot be empty unless flush=true",
        ));
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let state = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let assistant = state
        .get("assistant")
        .cloned()
        .or_else(|| state.get("voice_secretary").cloned())
        .unwrap_or_else(default_assistant);
    if !assistant["enabled"].as_bool().unwrap_or(false) {
        return Err(OpError::new(
            "assistant_disabled",
            "voice_secretary is disabled",
        ));
    }

    let now = utc_now();
    let language = string_arg(request, "language").unwrap_or_default();
    let is_final = bool_arg(request, "is_final", true);
    let document_path = effective_document_path(home, &group_id, request)?;
    let segment = json!({
        "schema":1,"segment_id":segment_id,"session_id":session_id,"group_id":group_id,
        "assistant_id":"voice_secretary","text":text,"language":language,"is_final":is_final,
        "start_ms":request.args.get("start_ms"),"end_ms":request.args.get("end_ms"),
        "speaker_label":string_arg(request,"speaker_label").unwrap_or_default(),
        "document_path":document_path,"trigger":request.args.get("trigger").cloned().unwrap_or_else(||json!({})),
        "by":string_arg(request,"by").unwrap_or_else(||"user".into()),"created_at":now,"updated_at":now
    });
    let segment_path = segment_log_path(home, &group_id, &session_id);
    ensure_document_file(home, &group, &document_path)?;
    let document_record = ensure_document_record(home, &group, &document_path, &now)?;

    let auto_document = with_transcript_lock(home, &group_id, || {
        let auto_document = assistant_state::update(home,&group_id,|root| {
            let sessions=array(root,"sessions");
            let index=sessions.iter().position(|item|item["session_id"]==session_id).unwrap_or_else(||{sessions.push(json!({"session_id":session_id,"capture_mode":"document","created_at":now,"segments":[],"transcript":""}));sessions.len()-1});
            let session=&mut sessions[index];
            let transcript={
                let segments=session.get_mut("segments").and_then(Value::as_array_mut).expect("segments initialized");
                let duplicate=segments.iter().any(|item|item["segment_id"]==segment_id);
                if !duplicate && !text.is_empty() {
                    segments.push(segment.clone());
                    if segments.len()>200 { segments.drain(..segments.len()-200); }
                }
                segments.iter().filter(|item|item["is_final"].as_bool().unwrap_or(true)).filter_map(|item|item["text"].as_str()).collect::<Vec<_>>().join("\n")
            };
            session["transcript"]=json!(transcript);
            session["updated_at"]=json!(now);
            session["document_path"]=json!(document_path);
            session["capture_mode"]=json!("document");
            session["language"]=json!(language);
            if !text.is_empty() && !segment_exists_io(&segment_path, &session_id, &segment_id)? {
                append_jsonl_io(&segment_path, &segment)?;
            }
            super::voice_session::prune_sessions(sessions);
            let auto_document=root.get("assistant").and_then(|item|item["config"]["auto_document_enabled"].as_bool()).unwrap_or(true);
            Ok(auto_document)
        })?;
        if is_final && !text.is_empty() {
            append_document_transcript(home, &group_id, &document_record, &segment, &segment_path)?;
        }
        Ok(auto_document)
    }).map_err(OpError::io)?;
    let input_kind = string_arg(request, "input_kind").unwrap_or_else(|| "asr_transcript".into());
    let (candidate_input, input_created) = if !is_final
        || text.is_empty()
        || (input_kind == "asr_transcript" && !auto_document)
    {
        find_input(home, &group_id, &session_id, &segment_id)
            .map(|existing| (existing, false))
            .map_err(OpError::io)?
    } else {
        append_input(home, &group_id, json!({
            "schema":1,
            "input_id":format!("vin_{}",Uuid::new_v4().simple()),
            "kind":input_kind,
            "group_id":group_id,
            "assistant_id":"voice_secretary",
            "text":text,
            "language":language,
            "document_path":document_path,
            "session_id":session_id,
            "segment_id":segment_id,
            "by":segment["by"],
            "trigger":segment["trigger"],
            "request_id":string_arg(request,"request_id").unwrap_or_default(),
            "operation":string_arg(request,"operation").unwrap_or_default(),
            "composer_snapshot_hash":string_arg(request,"composer_snapshot_hash").unwrap_or_default(),
            "metadata":request.args.get("metadata").cloned().unwrap_or_else(||json!({})),
            "created_at":now,
            "updated_at":now
        })).map_err(OpError::io)?
    };

    let delivery = voice_input_delivery::deliver(
        home,
        &store,
        &group_id,
        &session_id,
        &segment_id,
        segment["by"].as_str().unwrap_or("user"),
        candidate_input.as_ref(),
    )?;
    if delivery.notify.is_some() {
        if let Some(input) = candidate_input.as_ref() {
            mark_delivered(home, &group_id, input).map_err(OpError::io)?;
        }
    }
    let current = assistant_state::load(home, &group_id).map_err(OpError::io)?;
    let document_state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let document = document_state
        .get("documents")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["document_path"] == document_path)
        })
        .cloned();
    object(json!({
        "group_id":group_id,"assistant":current.get("assistant").cloned().unwrap_or_else(default_assistant),
        "session_id":session_id,"segment":segment,"segment_path":segment_path,"document":document,
        "document_updated":false,"input_event":candidate_input,"input_event_created":input_created,
        "event":delivery.event,"input_notify_event":delivery.notify,"input_notify_emitted":delivery.notify.is_some(),
        "actor_woken":delivery.actor_woken,"actor_wake_error":delivery.wake_error,
        "actor_notify_delivered":delivery.delivery.as_ref().and_then(|item|item["queued"].as_u64()).unwrap_or(0)>0,
        "actor_notify_delivery":delivery.delivery
    }))
}

pub fn named(home: &HomeLayout, request: &DaemonRequest, kind: &str, text: String) -> OpResult {
    voice_semantic_input::append(home, request, kind, text)
}

pub fn read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| "assistant:voice_secretary".into());
    if !matches!(by.as_str(), ACTOR_ID | "assistant:voice_secretary") {
        return Err(OpError::new(
            "assistant_voice_document_input_read_failed",
            "read_new_input is only available to voice-secretary",
        ));
    }
    let inputs = take_unread(home, &group_id).map_err(OpError::io)?;
    let document_state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let mut grouped = BTreeMap::<String, Vec<&Value>>::new();
    for item in &inputs {
        grouped
            .entry(item["document_path"].as_str().unwrap_or("").into())
            .or_default()
            .push(item);
    }
    let batches=grouped.into_iter().map(|(path,items)|{
        let kinds=items.iter().filter_map(|item|item["kind"].as_str()).collect::<BTreeSet<_>>();
        let languages=items.iter().filter_map(|item|item["language"].as_str()).filter(|v|!v.is_empty()).collect::<BTreeSet<_>>();
        json!({"document_path":path,"filename":Path::new(&path).file_name().and_then(|v|v.to_str()).unwrap_or(""),"item_count":items.len(),"kinds":kinds,"languages":languages,"items":items})
    }).collect::<Vec<_>>();
    let input_text = inputs
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    object(
        json!({"group_id":group_id,"item_count":inputs.len(),"document_count":batches.len(),"input_text":input_text,"input_batches":batches,"documents":document_state["documents"],"has_new_input":false}),
    )
}

fn effective_document_path(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
) -> Result<String, OpError> {
    let state = voice_document_state::load(home, group_id).map_err(OpError::io)?;
    let path = string_arg(request, "document_path")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| voice_document_state::active_path(&state).map(str::to_owned))
        .unwrap_or_else(|| {
            format!(
                "docs/voice-secretary/{}.md",
                chrono::Utc::now().format("%Y-%m-%d")
            )
        });
    validate_document_path(&path)?;
    Ok(path)
}

fn ensure_document_record(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    document_path: &str,
    now: &str,
) -> Result<Value, OpError> {
    voice_document_state::update(home, &group.group_id, |state| {
        let documents = array(state, "documents");
        let document = if let Some(document) = documents
            .iter()
            .find(|document| document["document_path"] == document_path)
            .cloned()
        {
            document
        } else {
            let title = Path::new(document_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Voice notes");
            let document = json!({
                "document_id":format!("vdoc_{}",Uuid::new_v4().simple()),
                "document_path":document_path,
                "workspace_path":document_path,
                "filename":Path::new(document_path).file_name().and_then(|value|value.to_str()).unwrap_or("notes.md"),
                "assistant_id":"voice_secretary",
                "title":title,
                "status":"active",
                "storage_kind":if group.scopes.is_empty(){"rust_home"}else{"workspace"},
                "content":"",
                "content_sha256":format!("{:x}",Sha256::digest(b"")),
                "content_chars":0,
                "revision_count":0,
                "created_at":now,
                "updated_at":now,
                "created_by":"user"
            });
            documents.push(document.clone());
            document
        };
        state.insert("active_document_path".into(), json!(document_path));
        state.insert("active_document_id".into(), document["document_id"].clone());
        Ok(document)
    })
    .map_err(OpError::io)
}

fn append_document_transcript(
    home: &HomeLayout,
    group_id: &str,
    document: &Value,
    segment: &Value,
    segment_path: &Path,
) -> std::io::Result<()> {
    let document_id = document["document_id"].as_str().unwrap_or_default().trim();
    let text = segment["text"].as_str().unwrap_or_default().trim();
    if document_id.is_empty() || text.is_empty() {
        return Ok(());
    }
    let path = voice_root(home, group_id)
        .join("documents")
        .join(document_id)
        .join("transcript.jsonl");
    let session_id = segment["session_id"].as_str().unwrap_or_default();
    let segment_id = segment["segment_id"].as_str().unwrap_or_default();
    if find_segment_io(&path, session_id, segment_id)?.is_some() {
        return Ok(());
    }
    let mut row = segment.clone();
    row["document_id"] = json!(document_id);
    row["document_path"] = document["document_path"].clone();
    row["segment_path"] = json!(segment_path.to_string_lossy());
    append_jsonl_io(&path, &row)
}
pub(super) fn validate_document_path(value: &str) -> Result<(), OpError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.extension().and_then(|value| value.to_str()) != Some("md")
    {
        Err(OpError::new(
            "invalid_args",
            "document_path must be a repository-relative Markdown path",
        ))
    } else {
        Ok(())
    }
}
pub(super) fn safe_id(value: &str) -> Result<String, OpError> {
    let value = value.trim();
    if value.is_empty()
        || matches!(value, "." | "..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Err(OpError::new(
            "invalid_args",
            "voice session/segment id must be one safe path component",
        ))
    } else {
        Ok(value.into())
    }
}
fn voice_root(home: &HomeLayout, group_id: &str) -> PathBuf {
    home.root().join("voice-secretary").join(group_id)
}
pub(super) fn with_transcript_lock<T>(
    home: &HomeLayout,
    group_id: &str,
    operation: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    cccc_core::fs::with_exclusive_lock(
        &voice_root(home, group_id).join("transcript.lock"),
        operation,
    )
}
fn segment_log_path(home: &HomeLayout, group_id: &str, session_id: &str) -> PathBuf {
    voice_root(home, group_id)
        .join(session_id)
        .join("transcripts/segments.jsonl")
}
pub(super) fn input_log_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    voice_root(home, group_id).join("input_events.jsonl")
}
fn legacy_input_log_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    voice_root(home, group_id).join("inputs.jsonl")
}
fn input_state_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    voice_root(home, group_id).join("input_state.json")
}
fn input_state_lock_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    voice_root(home, group_id).join("input_state.json.lock")
}
fn default_input_state(group_id: &str) -> Map<String, Value> {
    json!({
        "schema":INPUT_STATE_SCHEMA,
        "group_id":group_id,
        "latest_seq":0,
        "secretary_read_cursor":0,
        "secretary_delivery_cursor":0,
        "last_notify_at":"",
        "retry_count":0,
        "flush_count_since_idle_review":0,
        "last_idle_review_at":"",
        "last_idle_review_input_seq":0,
        "last_input_appended_at":"",
        "last_notify_emitted_at":"",
        "last_input_envelope_at":"",
        "last_input_envelope_id":"",
        "last_read_new_input_at":""
    })
    .as_object()
    .cloned()
    .expect("input state object")
}
fn load_input_state_unlocked(
    home: &HomeLayout,
    group_id: &str,
) -> std::io::Result<Map<String, Value>> {
    let path = input_state_path(home, group_id);
    let mut state = if path.is_file() {
        let value = serde_json::from_slice::<Value>(&std::fs::read(path)?)
            .map_err(std::io::Error::other)?;
        value
            .as_object()
            .cloned()
            .unwrap_or_else(|| default_input_state(group_id))
    } else {
        default_input_state(group_id)
    };
    if state.get("schema").and_then(Value::as_u64) != Some(INPUT_STATE_SCHEMA) {
        state = default_input_state(group_id);
    }
    state.insert("schema".into(), json!(INPUT_STATE_SCHEMA));
    state.insert("group_id".into(), json!(group_id));
    for (key, fallback) in [
        ("latest_seq", json!(0)),
        ("secretary_read_cursor", json!(0)),
        ("secretary_delivery_cursor", json!(0)),
    ] {
        if !state.get(key).is_some_and(Value::is_number) {
            state.insert(key.into(), fallback);
        }
    }
    Ok(state)
}
fn save_input_state_unlocked(
    home: &HomeLayout,
    group_id: &str,
    state: &Map<String, Value>,
) -> std::io::Result<()> {
    let mut bytes =
        serde_json::to_vec_pretty(&Value::Object(state.clone())).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    cccc_core::fs::atomic_write(&input_state_path(home, group_id), &bytes)
}
fn with_input_state_lock<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let path = input_state_lock_path(home, group_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    let result = change();
    let unlock = FileExt::unlock(&file);
    result.and_then(|value| unlock.map(|()| value))
}
fn input_event_key(value: &Value) -> Option<String> {
    let session_id = value["session_id"].as_str().unwrap_or_default().trim();
    let segment_id = value["segment_id"].as_str().unwrap_or_default().trim();
    if !session_id.is_empty() && !segment_id.is_empty() {
        return Some(format!("segment\0{session_id}\0{segment_id}"));
    }
    for field in ["input_append_id", "input_id"] {
        if let Some(value) = value[field]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(format!("{field}\0{value}"));
        }
    }
    None
}
fn covered_cursor(mut coverage: Vec<(u64, bool)>) -> u64 {
    coverage.sort_unstable_by_key(|(seq, _)| *seq);
    let mut cursor = 0;
    for (seq, covered) in coverage {
        if !covered {
            break;
        }
        cursor = seq;
    }
    cursor
}
fn migrate_legacy_input(home: &HomeLayout, group_id: &str) -> std::io::Result<()> {
    let legacy_path = legacy_input_log_path(home, group_id);
    if !legacy_path.is_file() {
        return Ok(());
    }
    let legacy = read_jsonl_matching(&legacy_path, |_| true)?;
    let legacy_state = assistant_state::load(home, group_id)?;
    let legacy_read_cursor = legacy_state["input_read_cursor"].as_u64().unwrap_or(0);

    with_input_state_lock(home, group_id, || {
        let canonical = read_jsonl_matching(&input_log_path(home, group_id), |_| true)?;
        let mut state = load_input_state_unlocked(home, group_id)?;
        let canonical_read = state
            .get("secretary_read_cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let canonical_delivery = state
            .get("secretary_delivery_cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .max(canonical_read);
        let mut latest = state.get("latest_seq").and_then(Value::as_u64).unwrap_or(0);
        let mut key_to_index = HashMap::new();
        let mut read_coverage = Vec::new();
        let mut delivery_coverage = Vec::new();
        for (index, value) in canonical.iter().enumerate() {
            let seq = value["seq"].as_u64().unwrap_or(0);
            latest = latest.max(seq);
            read_coverage.push((seq, seq > 0 && seq <= canonical_read));
            delivery_coverage.push((seq, seq > 0 && seq <= canonical_delivery));
            if let Some(key) = input_event_key(value) {
                key_to_index.insert(key, index);
            }
        }
        for mut value in legacy {
            let legacy_seq = value["seq"].as_u64().unwrap_or(0);
            let was_read = legacy_seq > 0 && legacy_seq <= legacy_read_cursor;
            if let Some(index) =
                input_event_key(&value).and_then(|key| key_to_index.get(&key).copied())
            {
                if was_read {
                    read_coverage[index].1 = true;
                    delivery_coverage[index].1 = true;
                }
                continue;
            }
            latest += 1;
            value["seq"] = json!(latest);
            let index = read_coverage.len();
            if let Some(key) = input_event_key(&value) {
                key_to_index.insert(key, index);
            }
            append_jsonl_io(&input_log_path(home, group_id), &value)?;
            read_coverage.push((latest, was_read));
            delivery_coverage.push((latest, was_read));
        }
        state.insert("latest_seq".into(), json!(latest));
        state.insert(
            "secretary_read_cursor".into(),
            json!(covered_cursor(read_coverage)),
        );
        state.insert(
            "secretary_delivery_cursor".into(),
            json!(covered_cursor(delivery_coverage)),
        );
        save_input_state_unlocked(home, group_id, &state)
    })?;
    // Canonical log/state commit first. If cleanup fails, the next attempt
    // deduplicates the still-present legacy log and cannot resurrect work.
    assistant_state::update(home, group_id, |state| {
        for key in ["input_latest_seq", "input_read_cursor", "input_updated_at"] {
            state.remove(key);
        }
        Ok(())
    })?;
    std::fs::remove_file(legacy_path)
}
pub(super) fn find_input(
    home: &HomeLayout,
    group_id: &str,
    session_id: &str,
    segment_id: &str,
) -> std::io::Result<Option<Value>> {
    migrate_legacy_input(home, group_id)?;
    find_segment_io(&input_log_path(home, group_id), session_id, segment_id)
}
pub(super) fn append_input(
    home: &HomeLayout,
    group_id: &str,
    mut record: Value,
) -> std::io::Result<(Option<Value>, bool)> {
    migrate_legacy_input(home, group_id)?;
    with_input_state_lock(home, group_id, || {
        let session_id = record["session_id"].as_str().unwrap_or_default();
        let segment_id = record["segment_id"].as_str().unwrap_or_default();
        let values = read_jsonl_matching(&input_log_path(home, group_id), |_| true)?;
        let mut state = load_input_state_unlocked(home, group_id)?;
        let log_latest = values
            .iter()
            .filter_map(|item| item["seq"].as_u64())
            .max()
            .unwrap_or(0);
        if let Some(existing) = values
            .iter()
            .rev()
            .find(|item| item["session_id"] == session_id && item["segment_id"] == segment_id)
            .cloned()
        {
            let latest = state
                .get("latest_seq")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .max(log_latest);
            state.insert("latest_seq".into(), json!(latest));
            save_input_state_unlocked(home, group_id, &state)?;
            return Ok((Some(existing), false));
        }
        let next_seq = state
            .get("latest_seq")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .max(log_latest)
            + 1;
        record["seq"] = json!(next_seq);
        append_jsonl_io(&input_log_path(home, group_id), &record)?;
        state.insert("latest_seq".into(), json!(next_seq));
        state.insert(
            "last_input_appended_at".into(),
            record
                .get("created_at")
                .cloned()
                .unwrap_or_else(|| json!(utc_now())),
        );
        save_input_state_unlocked(home, group_id, &state)?;
        Ok((Some(record), true))
    })
}
fn take_unread(home: &HomeLayout, group_id: &str) -> std::io::Result<Vec<Value>> {
    migrate_legacy_input(home, group_id)?;
    with_input_state_lock(home, group_id, || {
        let mut state = load_input_state_unlocked(home, group_id)?;
        let cursor = state
            .get("secretary_read_cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let inputs = read_jsonl_matching(&input_log_path(home, group_id), |item| {
            item["seq"].as_u64().unwrap_or(0) > cursor
        })?;
        let latest = inputs
            .iter()
            .filter_map(|item| item["seq"].as_u64())
            .max()
            .unwrap_or(cursor);
        if latest > cursor {
            state.insert("secretary_read_cursor".into(), json!(latest));
            let delivered = state
                .get("secretary_delivery_cursor")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .max(latest);
            state.insert("secretary_delivery_cursor".into(), json!(delivered));
        }
        state.insert("last_read_new_input_at".into(), json!(utc_now()));
        save_input_state_unlocked(home, group_id, &state)?;
        Ok(inputs)
    })
}
pub(super) fn mark_delivered(
    home: &HomeLayout,
    group_id: &str,
    input: &Value,
) -> std::io::Result<()> {
    let seq = input["seq"].as_u64().unwrap_or(0);
    if seq == 0 {
        return Ok(());
    }
    migrate_legacy_input(home, group_id)?;
    with_input_state_lock(home, group_id, || {
        let mut state = load_input_state_unlocked(home, group_id)?;
        let delivered = state
            .get("secretary_delivery_cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .max(seq);
        state.insert("secretary_delivery_cursor".into(), json!(delivered));
        state.insert("last_notify_emitted_at".into(), json!(utc_now()));
        save_input_state_unlocked(home, group_id, &state)
    })
}
pub(super) fn status(home: &HomeLayout, group_id: &str) -> std::io::Result<(u64, u64)> {
    migrate_legacy_input(home, group_id)?;
    with_input_state_lock(home, group_id, || {
        let state = load_input_state_unlocked(home, group_id)?;
        let latest = state.get("latest_seq").and_then(Value::as_u64).unwrap_or(0);
        let covered = state
            .get("secretary_read_cursor")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .max(
                state
                    .get("secretary_delivery_cursor")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        Ok((latest, covered))
    })
}
fn ensure_document_file(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    relative: &str,
) -> Result<(), OpError> {
    if relative.is_empty() {
        return Ok(());
    }
    let path = if let Some(scope) = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
    {
        let root = Path::new(&scope.url).canonicalize().map_err(OpError::io)?;
        checked_document_path(&root, relative)?
    } else {
        let root = voice_root(home, &group.group_id).join("documents");
        std::fs::create_dir_all(&root).map_err(OpError::io)?;
        checked_document_path(&root, relative)?
    };
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(OpError::io)?;
    }
    std::fs::write(path, b"").map_err(OpError::io)
}
pub(super) fn append_jsonl_io(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    let result = (|| {
        repair_incomplete_tail_locked(&mut file)?;
        let mut bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        file.seek(SeekFrom::End(0))?;
        file.write_all(&bytes)?;
        file.sync_data()
    })();
    let unlock = FileExt::unlock(&file);
    result.and(unlock)
}

fn repair_incomplete_tail_locked(file: &mut std::fs::File) -> std::io::Result<()> {
    const CHUNK_BYTES: usize = 64 * 1024;
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(());
    }

    let mut position = len;
    let mut chunks = Vec::new();
    let truncate_at;
    loop {
        let chunk_len = position.min(CHUNK_BYTES as u64) as usize;
        position -= chunk_len as u64;
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = vec![0; chunk_len];
        file.read_exact(&mut chunk)?;
        if let Some(newline) = chunk.iter().rposition(|byte| *byte == b'\n') {
            truncate_at = position + newline as u64 + 1;
            chunks.push(chunk[newline + 1..].to_vec());
            break;
        }
        chunks.push(chunk);
        if position == 0 {
            truncate_at = 0;
            break;
        }
    }
    chunks.reverse();
    let tail = chunks.concat();
    if tail.iter().all(u8::is_ascii_whitespace) || serde_json::from_slice::<Value>(&tail).is_ok() {
        file.seek(SeekFrom::End(0))?;
        file.write_all(b"\n")?;
    } else {
        file.set_len(truncate_at)?;
    }
    file.sync_data()
}
pub(super) fn read_jsonl_matching(
    path: &Path,
    include: impl Fn(&Value) -> bool,
) -> std::io::Result<Vec<Value>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.lock_exclusive()?;
    let result = (|| {
        repair_incomplete_tail_locked(&mut file)?;
        file.seek(SeekFrom::Start(0))?;
        let mut values = Vec::new();
        let mut line = Vec::new();
        let mut reader = BufReader::new(&mut file);
        while reader.read_until(b'\n', &mut line)? > 0 {
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            let trimmed = line.as_slice();
            if !trimmed.iter().all(u8::is_ascii_whitespace) {
                let value = serde_json::from_slice(trimmed).map_err(std::io::Error::other)?;
                if include(&value) {
                    values.push(value);
                }
            }
            line.clear();
        }
        Ok(values)
    })();
    let unlock = FileExt::unlock(&file);
    result.and_then(|values| unlock.map(|()| values))
}
fn segment_exists_io(path: &Path, session_id: &str, segment_id: &str) -> std::io::Result<bool> {
    Ok(find_segment_io(path, session_id, segment_id)?.is_some())
}
pub(super) fn find_segment_io(
    path: &Path,
    session_id: &str,
    segment_id: &str,
) -> std::io::Result<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    read_jsonl_matching(path, |item| {
        item["session_id"] == session_id && item["segment_id"] == segment_id
    })
    .map(|mut values| values.pop())
}
fn checked_document_path(root: &Path, relative: &str) -> Result<PathBuf, OpError> {
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(OpError::new("invalid_args", "invalid document_path"));
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(OpError::new(
                    "invalid_args",
                    "document_path must not traverse symbolic links",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(OpError::io(error)),
        }
    }
    Ok(current)
}
fn array<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("array initialized")
}
pub(super) fn default_assistant() -> Value {
    json!({"assistant_id":"voice_secretary","kind":"voice_secretary","enabled":false,"lifecycle":"disabled","config":{"auto_document_enabled":true}})
}

#[allow(dead_code)]
fn content_sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_repairs_only_the_incomplete_tail() {
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(file.path(), b"{\"seq\":1}\n{\"seq\":").expect("fixture");

        append_jsonl_io(file.path(), &json!({"seq":2})).expect("append");

        let values = read_jsonl_matching(file.path(), |_| true).expect("read repaired log");
        assert_eq!(values, [json!({"seq":1}), json!({"seq":2})]);
    }

    #[test]
    fn append_preserves_a_valid_final_record_without_newline() {
        let file = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(file.path(), b"{\"seq\":1}").expect("fixture");

        append_jsonl_io(file.path(), &json!({"seq":2})).expect("append");

        let values = read_jsonl_matching(file.path(), |_| true).expect("read log");
        assert_eq!(values, [json!({"seq":1}), json!({"seq":2})]);
    }
}

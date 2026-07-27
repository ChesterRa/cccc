use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, integration_state, ledger};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::{actor_delivery, actor_runtime};

const KEY: &str = "assistants";
const ACTOR_ID: &str = "voice-secretary";

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
    let state = integration_state::group_get(&store, &group_id, KEY).map_err(OpError::io)?;
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
    let document_path = effective_document_path(request, &state)?;
    let segment = json!({
        "schema":1,"segment_id":segment_id,"session_id":session_id,"group_id":group_id,
        "assistant_id":"voice_secretary","text":text,"language":language,"is_final":is_final,
        "start_ms":request.args.get("start_ms"),"end_ms":request.args.get("end_ms"),
        "speaker_label":string_arg(request,"speaker_label").unwrap_or_default(),
        "document_path":document_path,"trigger":request.args.get("trigger").cloned().unwrap_or_else(||json!({})),
        "by":string_arg(request,"by").unwrap_or_else(||"user".into()),"created_at":now,"updated_at":now
    });
    let segment_path = segment_log_path(home, &group_id, &session_id);
    let input_path = input_log_path(home, &group_id);
    let needs_notice = group.actors.iter().any(|actor| actor.id == ACTOR_ID);
    ensure_document_file(home, &group, &document_path)?;

    let (candidate_input, input_created) = integration_state::group_update(&store,&group_id,KEY,|value| {
        let root=state_root(value);
        if !document_path.is_empty() {
            let docs=array(root,"documents");
            if !docs.iter().any(|item|item["document_path"]==document_path) {
                let title=Path::new(&document_path).file_stem().and_then(|value|value.to_str()).unwrap_or("Voice notes");
                docs.push(json!({"document_id":format!("vdoc_{}",Uuid::new_v4().simple()),"document_path":document_path,"workspace_path":document_path,"filename":Path::new(&document_path).file_name().and_then(|value|value.to_str()).unwrap_or("notes.md"),"assistant_id":"voice_secretary","title":title,"status":"active","storage_kind":if group.scopes.is_empty(){"rust_home"}else{"workspace"},"content":"","content_sha256":format!("{:x}",Sha256::digest(b"")),"content_chars":0,"revision_count":0,"created_at":now,"updated_at":now,"created_by":"user"}));
            }
            let selected=docs.iter().find(|item|item["document_path"]==document_path).cloned().unwrap_or(Value::Null);
            root.insert("active_document_path".into(),json!(document_path));
            root.insert("active_document_id".into(),selected["document_id"].clone());
        }
        let sessions=array(root,"sessions");
        let index=sessions.iter().position(|item|item["session_id"]==session_id).unwrap_or_else(||{sessions.push(json!({"session_id":session_id,"created_at":now,"segments":[],"transcript":""}));if sessions.len()>50{sessions.remove(0);}sessions.len()-1});
        let session=&mut sessions[index];
        let speaker_ranges=session["diarization"]["segments"].as_array().cloned().unwrap_or_default();
        let (duplicate,speaker_transcript_segments,transcript)={
            let segments=session.get_mut("segments").and_then(Value::as_array_mut).expect("segments initialized");
            let duplicate=segments.iter().any(|item|item["segment_id"]==segment_id);
            if !duplicate && !text.is_empty() {
                let mut stored_segment=segment.clone();
                apply_speaker_range(&mut stored_segment,&speaker_ranges);
                segments.push(stored_segment);
                if segments.len()>200 { segments.drain(..segments.len()-200); }
            }
            let speaker_segments=(!speaker_ranges.is_empty()).then(||segments.clone());
            let transcript=segments.iter().filter(|item|item["is_final"].as_bool().unwrap_or(true)).filter_map(|item|item["text"].as_str()).collect::<Vec<_>>().join("\n");
            (duplicate,speaker_segments,transcript)
        };
        session["transcript"]=json!(transcript);
        if let Some(segments)=speaker_transcript_segments {
            session["diarization"]["speaker_transcript_segments"]=json!(segments);
        }
        session["updated_at"]=json!(now);
        session["document_path"]=json!(document_path);
        if !text.is_empty() && !segment_exists_io(&segment_path, &session_id, &segment_id)? {
            append_jsonl_io(&segment_path, &segment)?;
        }
        if let Some(existing)=find_segment_io(&input_path, &session_id, &segment_id)? {
            let seq=existing["seq"].as_u64().unwrap_or(0);
            let latest=root.get("input_latest_seq").and_then(Value::as_u64).unwrap_or(0);
            root.insert("input_latest_seq".into(),json!(latest.max(seq)));
            return Ok((Some(existing), false));
        }
        if duplicate || !is_final || text.is_empty() { return Ok((None, false)); }
        let input_kind=string_arg(request,"input_kind").unwrap_or_else(||"asr_transcript".into());
        let auto_document=root.get("assistant").and_then(|item|item["config"]["auto_document_enabled"].as_bool()).unwrap_or(true);
        if input_kind=="asr_transcript" && !auto_document { return Ok((None, false)); }
        let next_seq=root.get("input_latest_seq").and_then(Value::as_u64).unwrap_or(0)+1;
        let record=json!({"schema":1,"seq":next_seq,"input_id":format!("vin_{}",Uuid::new_v4().simple()),"kind":input_kind,"text":text,"language":language,"document_path":document_path,"session_id":session_id,"segment_id":segment_id,"by":segment["by"],"trigger":segment["trigger"],"created_at":now});
        append_jsonl_io(&input_path, &record)?;
        root.insert("input_latest_seq".into(),json!(next_seq));
        root.insert("input_updated_at".into(),json!(now));
        Ok((Some(record), true))
    }).map_err(OpError::io)?;

    let prior_events = if candidate_input.is_some() {
        voice_events_for_segment(&store, &group_id, &session_id, &segment_id)?
    } else {
        (None, None)
    };
    let prior_delivery_complete =
        prior_events.0.is_some() && (!needs_notice || prior_events.1.is_some());
    let input = (!prior_delivery_complete)
        .then_some(candidate_input.clone())
        .flatten();

    let mut event = None;
    let mut notify = None;
    let mut delivery = None;
    let mut actor_woken = false;
    let mut wake_error = String::new();
    if let Some(ref input) = input {
        let ledger_path = store.ledger_path(&group_id).map_err(OpError::io)?;
        let input_event = if let Some(event) = prior_events.0 {
            event
        } else {
            let mut event = Event::new("assistant.voice.input", &group_id);
            event.by = segment["by"].as_str().unwrap_or("user").into();
            event.data = input.as_object().cloned().unwrap_or_default();
            ledger::append(&ledger_path, &event).map_err(OpError::io)?;
            event
        };
        event = Some(input_event);

        let latest = store.load(&group_id).map_err(OpError::not_found)?;
        if latest.actors.iter().any(|actor| actor.id == ACTOR_ID) {
            if cccc_runtime::status(&group_id, ACTOR_ID).is_ok_and(|status| status.running) {
                actor_woken = true;
            } else if latest.running {
                match actor_runtime::apply(home, &latest, ACTOR_ID, "actor.start") {
                    Ok(status) => actor_woken = status.is_some_and(|item| item.running),
                    Err(error) => wake_error = format!("{}: {}", error.code, error.message),
                }
            }
            let notice = if let Some(event) = prior_events.1 {
                event
            } else {
                let mut event = Event::new("system.notify", &group_id);
                event.by = "system".into();
                event.data=json!({"kind":"voice_secretary_input","title":"Voice Secretary input","text":"New voice input is ready.","to":[ACTOR_ID],"priority":"normal","requires_ack":false,"context":{"kind":"voice_secretary_input","input_envelope":input}}).as_object().cloned().unwrap_or_default();
                ledger::append(&ledger_path, &event).map_err(OpError::io)?;
                event
            };
            let report = actor_delivery::dispatch(home, &latest, &notice);
            delivery = serde_json::to_value(report).ok();
            notify = Some(notice);
        }
    }
    let current = integration_state::group_get(&store, &group_id, KEY).map_err(OpError::io)?;
    let document = current
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
        "event":event,"input_notify_event":notify,"input_notify_emitted":notify.is_some(),
        "actor_woken":actor_woken,"actor_wake_error":wake_error,
        "actor_notify_delivered":delivery.as_ref().and_then(|item|item["queued"].as_u64()).unwrap_or(0)>0,
        "actor_notify_delivery":delivery
    }))
}

pub fn instruction(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let instruction = required_arg(request, "instruction")?;
    named(home, request, "voice_instruction", instruction)
}

pub fn named(home: &HomeLayout, request: &DaemonRequest, kind: &str, text: String) -> OpResult {
    let mut forwarded = request.clone();
    forwarded.args.insert(
        "session_id".into(),
        json!(format!("input-{}", Uuid::new_v4().simple())),
    );
    forwarded.args.insert(
        "segment_id".into(),
        json!(format!("input-{}", Uuid::new_v4().simple())),
    );
    forwarded.args.insert("text".into(), json!(text));
    forwarded.args.insert("is_final".into(), json!(true));
    forwarded.args.insert("flush".into(), json!(false));
    forwarded.args.insert("input_kind".into(), json!(kind));
    forwarded
        .args
        .entry("trigger")
        .or_insert_with(|| json!({"trigger_kind":"user_instruction","source":"user"}));
    append(home, &forwarded)
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
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let state = integration_state::group_get(&store, &group_id, KEY).map_err(OpError::io)?;
    let cursor = state["input_read_cursor"].as_u64().unwrap_or(0);
    let inputs = read_jsonl_matching(&input_log_path(home, &group_id), |item| {
        item["seq"].as_u64().unwrap_or(0) > cursor
    })
    .map_err(OpError::io)?;
    let latest = inputs
        .iter()
        .filter_map(|item| item["seq"].as_u64())
        .max()
        .unwrap_or(cursor);
    if latest > cursor {
        integration_state::group_update(&store, &group_id, KEY, |value| {
            state_root(value).insert("input_read_cursor".into(), json!(latest));
            Ok(())
        })
        .map_err(OpError::io)?;
    }
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
        json!({"group_id":group_id,"item_count":inputs.len(),"document_count":batches.len(),"input_text":input_text,"input_batches":batches,"documents":state["documents"],"has_new_input":false}),
    )
}

fn effective_document_path(request: &DaemonRequest, state: &Value) -> Result<String, OpError> {
    let path = string_arg(request, "document_path")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            state["active_document_path"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| {
            format!(
                "docs/voice-secretary/{}.md",
                chrono::Utc::now().format("%Y-%m-%d")
            )
        });
    validate_document_path(&path)?;
    Ok(path)
}
fn validate_document_path(value: &str) -> Result<(), OpError> {
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
fn safe_id(value: &str) -> Result<String, OpError> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Err(OpError::new(
            "invalid_args",
            "voice session/segment id contains unsupported characters",
        ))
    } else {
        Ok(value.into())
    }
}
fn voice_root(home: &HomeLayout, group_id: &str) -> PathBuf {
    home.root().join("voice-secretary").join(group_id)
}
fn segment_log_path(home: &HomeLayout, group_id: &str, session_id: &str) -> PathBuf {
    voice_root(home, group_id)
        .join(session_id)
        .join("transcripts/segments.jsonl")
}
fn input_log_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    voice_root(home, group_id).join("inputs.jsonl")
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
fn append_jsonl_io(path: &Path, value: &Value) -> std::io::Result<()> {
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
fn read_jsonl_matching(
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
fn find_segment_io(
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
fn voice_events_for_segment(
    store: &GroupStore,
    group_id: &str,
    session_id: &str,
    segment_id: &str,
) -> Result<(Option<Event>, Option<Event>), OpError> {
    let events = ledger::read_all(&store.ledger_path(group_id).map_err(OpError::io)?)
        .map_err(OpError::io)?;
    let input = events
        .iter()
        .find(|event| {
            event.kind == "assistant.voice.input"
                && event_data_string(event, &["session_id"]) == Some(session_id)
                && event_data_string(event, &["segment_id"]) == Some(segment_id)
        })
        .cloned();
    let notice = events
        .iter()
        .find(|event| {
            event.kind == "system.notify"
                && event_data_string(event, &["kind"]) == Some("voice_secretary_input")
                && event_data_string(event, &["context", "input_envelope", "session_id"])
                    == Some(session_id)
                && event_data_string(event, &["context", "input_envelope", "segment_id"])
                    == Some(segment_id)
        })
        .cloned();
    Ok((input, notice))
}
fn event_data_string<'a>(event: &'a Event, path: &[&str]) -> Option<&'a str> {
    let (first, rest) = path.split_first()?;
    let mut value = event.data.get(*first)?;
    for key in rest {
        value = value.get(*key)?;
    }
    value.as_str()
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
fn apply_speaker_range(segment: &mut Value, ranges: &[Value]) {
    let start = segment["start_ms"].as_i64().unwrap_or(0);
    let end = segment["end_ms"].as_i64().unwrap_or(start);
    let midpoint = start.saturating_add(end).saturating_div(2);
    if let Some(range) = ranges.iter().find(|range| {
        range["start_ms"].as_i64().unwrap_or(i64::MAX) <= midpoint
            && range["end_ms"].as_i64().unwrap_or(i64::MIN) >= midpoint
    }) {
        segment["speaker_index"] = range["speaker"].clone();
        segment["speaker_label"] = range["speaker_label"].clone();
    }
}
fn state_root(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("assistant state initialized");
    let legacy = root.get("voice_secretary").cloned();
    root.entry("assistant")
        .or_insert_with(|| legacy.unwrap_or_else(default_assistant));
    for key in ["documents", "sessions", "ask_requests"] {
        root.entry(key).or_insert_with(|| json!([]));
    }
    root
}
fn array<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("array initialized")
}
fn default_assistant() -> Value {
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

use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

const KEY: &str = "assistants";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "assistant_voice_document_list" => documents(home, request),
        "assistant_voice_document_select" => select(home, request),
        "assistant_voice_document_input_read" => read(home, request),
        "assistant_voice_document_save" => save(home, request),
        "assistant_voice_document_instruction" => instruction(home, request),
        "assistant_voice_document_archive" => archive(home, request),
        "assistant_voice_prompt_draft_submit" => prompt_submit(home, request),
        "assistant_voice_prompt_draft_ack" => prompt_ack(home, request),
        "assistant_voice_instruction_feedback" => feedback(home, request),
        "assistant_voice_ask_requests_clear" => clear(home, request),
        "assistant_voice_request" => voice_request(home, request),
        _ => return None,
    })
}

fn documents(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let value = load(home, &group_id)?;
    object(
        json!({"group_id":group_id,"documents":items(&value,"documents"),"active_document_id":value["active_document_id"],"active_document_path":value["active_document_path"]}),
    )
}
fn select(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    let document = update(home, &group_id, |state| {
        let document = array(state, "documents")
            .iter()
            .find(|item| item["document_path"] == path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        state.insert("active_document_id".into(), document["document_id"].clone());
        state.insert("active_document_path".into(), json!(path));
        Ok(document)
    })?;
    document_result(&group_id, document)
}
fn read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let value = load(home, &group_id)?;
    let path = string_arg(request, "document_path")
        .unwrap_or_else(|| value["active_document_path"].as_str().unwrap_or("").into());
    let document = items(&value, "documents")
        .iter()
        .find(|item| item["document_path"] == path)
        .cloned()
        .ok_or_else(|| OpError::new("not_found", "document not found"))?;
    document_result(&group_id, document)
}
fn save(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = string_arg(request, "document_path")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("voice/{}.md", short_id()));
    validate_path(&path)?;
    let title = string_arg(request, "title").unwrap_or_default();
    let content = string_arg(request, "content");
    let document = update(home, &group_id, |state| {
        let docs = array(state, "documents");
        let index = docs.iter().position(|item| item["document_path"] == path);
        let old = index
            .and_then(|index| docs.get(index))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let text = content
            .as_deref()
            .unwrap_or_else(|| old["content"].as_str().unwrap_or(""));
        let document = json!({"document_id":old["document_id"].as_str().map(str::to_owned).unwrap_or_else(||format!("vdoc_{}",short_id())),"document_path":path,"workspace_path":path,"filename":path.rsplit('/').next().unwrap_or(&path),"assistant_id":"voice_secretary","title":if title.is_empty(){old["title"].as_str().unwrap_or("Untitled document")}else{&title},"status":old["status"].as_str().unwrap_or("active"),"storage_kind":"rust_home","content":text,"content_sha256":format!("{:x}",Sha256::digest(text.as_bytes())),"content_chars":text.chars().count(),"revision_count":old["revision_count"].as_u64().unwrap_or(0)+1,"updated_at":utc_now()});
        if let Some(index) = index {
            docs[index] = document.clone();
        } else {
            docs.push(document.clone());
        }
        state.insert("active_document_id".into(), document["document_id"].clone());
        state.insert("active_document_path".into(), json!(path));
        Ok(document)
    })?;
    document_result(&group_id, document)
}
fn instruction(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let text = required_arg(request, "instruction")?;
    let path = document_path(request)?;
    let request = add_request(home, &group_id, &text, &path, "voice_instruction")?;
    object(json!({"group_id":group_id,"request_id":request["request_id"],"input_event":request}))
}
fn archive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    let document = update(home, &group_id, |state| {
        let item = array(state, "documents")
            .iter_mut()
            .find(|item| item["document_path"] == path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        item["status"] = json!("archived");
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    document_result(&group_id, document)
}
fn prompt_submit(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let text = string_arg(request, "text")
        .or_else(|| string_arg(request, "voice_transcript"))
        .or_else(|| string_arg(request, "composer_text"))
        .ok_or_else(|| OpError::new("invalid_args", "text is required"))?;
    let draft = update(home, &group_id, |state| {
        let draft = json!({"request_id":format!("vpr_{}",short_id()),"status":"pending","operation":string_arg(request,"operation").unwrap_or_else(||"refine".into()),"draft_text":text,"draft_preview":text,"created_at":utc_now()});
        state.insert("prompt_draft".into(), draft.clone());
        Ok(draft)
    })?;
    object(json!({"group_id":group_id,"prompt_draft":draft}))
}
fn prompt_ack(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let request_id = required_arg(request, "request_id")?;
    let status = required_arg(request, "status")?;
    let draft = update(home, &group_id, |state| {
        let draft = state
            .get_mut("prompt_draft")
            .filter(|draft| draft["request_id"] == request_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "prompt draft not found"))?;
        draft["status"] = json!(status);
        Ok(draft.clone())
    })?;
    object(json!({"group_id":group_id,"prompt_draft":draft}))
}
fn feedback(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let request_id = required_arg(request, "request_id")?;
    let status = string_arg(request, "status").unwrap_or_else(|| "completed".into());
    let item = update(home, &group_id, |state| {
        let item = array(state, "ask_requests")
            .iter_mut()
            .find(|item| item["request_id"] == request_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "request not found"))?;
        item["status"] = json!(status);
        item["reply_text"] = json!(string_arg(request, "reply_text").unwrap_or_default());
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    object(json!({"group_id":group_id,"request":item}))
}
fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    update(home, &group_id, |state| {
        array(state, "ask_requests").clear();
        Ok(())
    })?;
    object(json!({"group_id":group_id,"ask_requests":[]}))
}
fn voice_request(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let text = string_arg(request, "text")
        .or_else(|| string_arg(request, "instruction"))
        .ok_or_else(|| OpError::new("invalid_args", "text is required"))?;
    let item = add_request(
        home,
        &group_id,
        &text,
        &string_arg(request, "document_path").unwrap_or_default(),
        "ask",
    )?;
    object(json!({"group_id":group_id,"request":item}))
}
fn add_request(
    home: &HomeLayout,
    group_id: &str,
    text: &str,
    path: &str,
    kind: &str,
) -> Result<Value, OpError> {
    update(home, group_id, |state| {
        let item = json!({"request_id":format!("var_{}",short_id()),"kind":kind,"request_text":text,"document_path":path,"status":"pending","created_at":utc_now(),"updated_at":utc_now()});
        array(state, "ask_requests").push(item.clone());
        Ok(item)
    })
}
fn document_result(group_id: &str, document: Value) -> OpResult {
    object(
        json!({"group_id":group_id,"document":document,"input_event_created":false,"actor_woken":false}),
    )
}
fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_get(&store, group_id, KEY).map_err(OpError::io)
}
fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, group_id, KEY, |value| {
        if !value.is_object() {
            *value = json!({});
        }
        change(value.as_object_mut().expect("assistant state initialized"))
    })
    .map_err(OpError::io)
}
fn array<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("array initialized")
}
fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn document_path(request: &DaemonRequest) -> Result<String, OpError> {
    string_arg(request, "document_path")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "document_path is required"))
}
fn validate_path(value: &str) -> Result<(), OpError> {
    let path = std::path::Path::new(value);
    (!path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)))
    .then_some(())
    .ok_or_else(|| OpError::new("invalid_args", "invalid document_path"))
}
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}

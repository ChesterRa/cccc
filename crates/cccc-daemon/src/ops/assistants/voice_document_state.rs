use cccc_core::{HomeLayout, assistant_state};
use fs2::FileExt;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;

const SCHEMA: u64 = 1;

fn index_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    home.root()
        .join("voice-secretary")
        .join(group_id)
        .join("documents/index.json")
}

fn lock_path(home: &HomeLayout, group_id: &str) -> PathBuf {
    home.root()
        .join("voice-secretary")
        .join(group_id)
        .join("documents/index.json.lock")
}

fn empty_index(group_id: &str) -> Value {
    json!({"schema":SCHEMA,"group_id":group_id,"active_document_id":"","documents":{}})
}

fn load_index_unlocked(home: &HomeLayout, group_id: &str) -> io::Result<Value> {
    let path = index_path(home, group_id);
    let mut index = if path.is_file() {
        serde_json::from_slice::<Value>(&std::fs::read(path)?).map_err(io::Error::other)?
    } else {
        empty_index(group_id)
    };
    let Some(root) = index.as_object_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Voice Secretary document index must be an object",
        ));
    };
    if root.get("schema").and_then(Value::as_u64) != Some(SCHEMA) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported Voice Secretary document index schema",
        ));
    }
    let stored_group = root.get("group_id").and_then(Value::as_str).unwrap_or("");
    if !stored_group.is_empty() && stored_group != group_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Voice Secretary document index group_id mismatch",
        ));
    }
    root.insert("schema".into(), json!(SCHEMA));
    root.insert("group_id".into(), json!(group_id));
    if !root.get("active_document_id").is_some_and(Value::is_string) {
        root.insert("active_document_id".into(), json!(""));
    }
    if !root.get("documents").is_some_and(Value::is_object) {
        root.insert("documents".into(), json!({}));
    }
    Ok(index)
}

fn save_index_unlocked(home: &HomeLayout, group_id: &str, index: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(index).map_err(io::Error::other)?;
    bytes.push(b'\n');
    cccc_core::fs::atomic_write(&index_path(home, group_id), &bytes)
}

fn with_lock<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let path = lock_path(home, group_id);
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

fn document_path(document: &Value) -> &str {
    document["document_path"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| document["workspace_path"].as_str())
        .unwrap_or_default()
}

fn migrated_document_id(path: &str) -> String {
    format!("vdoc_migrated_{:x}", Sha256::digest(path.as_bytes()))
        .chars()
        .take(30)
        .collect()
}

fn merge_legacy(index: &mut Value, legacy: &Value) {
    let needs_active = index["active_document_id"]
        .as_str()
        .is_none_or(str::is_empty);
    let documents = index["documents"]
        .as_object_mut()
        .expect("document map normalized");
    let mut paths = documents
        .values()
        .map(document_path)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::HashSet<_>>();
    for document in legacy["documents"].as_array().into_iter().flatten() {
        let path = document_path(document);
        if path.is_empty() || paths.contains(path) {
            continue;
        }
        let mut id = document["document_id"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| migrated_document_id(path));
        if documents.contains_key(&id) {
            id = migrated_document_id(path);
        }
        let mut document = document.clone();
        document["document_id"] = json!(id);
        document["document_path"] = json!(path);
        documents.insert(id, document);
        paths.insert(path.to_owned());
    }
    if needs_active {
        let configured_id = legacy["active_document_id"].as_str().unwrap_or_default();
        let configured_path = legacy["active_document_path"].as_str().unwrap_or_default();
        let selected = documents
            .iter()
            .find(|(id, document)| {
                is_active(document)
                    && ((!configured_id.is_empty() && id.as_str() == configured_id)
                        || (!configured_path.is_empty()
                            && document_path(document) == configured_path))
            })
            .map(|(id, _)| id.clone());
        if let Some(id) = selected {
            index["active_document_id"] = json!(id);
        }
    }
}

fn migrate_legacy(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    let legacy = assistant_state::load(home, group_id)?;
    let has_documents = legacy["documents"]
        .as_array()
        .is_some_and(|documents| !documents.is_empty());
    let has_active = legacy["active_document_id"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
        || legacy["active_document_path"]
            .as_str()
            .is_some_and(|value| !value.is_empty());
    if !has_documents && !has_active {
        return Ok(());
    }
    with_lock(home, group_id, || {
        let mut index = load_index_unlocked(home, group_id)?;
        merge_legacy(&mut index, &legacy);
        save_index_unlocked(home, group_id, &index)
    })?;
    assistant_state::update(home, group_id, |state| {
        for key in ["documents", "active_document_id", "active_document_path"] {
            state.remove(key);
        }
        Ok(())
    })
}

fn flat_from_index(index: &Value) -> Value {
    let mut documents = index["documents"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, document)| {
            let mut document = document.clone();
            if document["document_id"].as_str().is_none_or(str::is_empty) {
                document["document_id"] = json!(id);
            }
            let path = document_path(&document).to_owned();
            document["document_path"] = json!(path);
            document
        })
        .collect::<Vec<_>>();
    documents.sort_by(|left, right| {
        updated_at(right)
            .cmp(updated_at(left))
            .then_with(|| document_path(right).cmp(document_path(left)))
    });
    let active_id = index["active_document_id"].as_str().unwrap_or_default();
    let active_path = documents
        .iter()
        .find(|document| document["document_id"] == active_id && is_active(document))
        .map(document_path)
        .unwrap_or_default();
    json!({
        "documents":documents,
        "active_document_id":active_id,
        "active_document_path":active_path
    })
}

fn index_from_flat(group_id: &str, state: &mut Map<String, Value>) -> Value {
    repair_active(state);
    let mut documents = Map::new();
    for document in state
        .get("documents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = document_path(document);
        if path.is_empty() {
            continue;
        }
        let id = document["document_id"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| migrated_document_id(path));
        let mut document = document.clone();
        document["document_id"] = json!(id);
        document["document_path"] = json!(path);
        documents.insert(id, document);
    }
    json!({
        "schema":SCHEMA,
        "group_id":group_id,
        "active_document_id":state.get("active_document_id").cloned().unwrap_or_else(||json!("")),
        "documents":documents
    })
}

pub(super) fn load(home: &HomeLayout, group_id: &str) -> io::Result<Value> {
    migrate_legacy(home, group_id)?;
    with_lock(home, group_id, || {
        load_index_unlocked(home, group_id).map(|index| flat_from_index(&index))
    })
}

pub(super) fn transcript_log(
    home: &HomeLayout,
    group_id: &str,
    requested_path: &str,
) -> io::Result<Option<(String, PathBuf)>> {
    let requested_path = requested_path.trim();
    if requested_path.is_empty() {
        return Ok(None);
    }
    let state = load(home, group_id)?;
    let Some(document_id) = state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|document| document_path(document) == requested_path)
        .and_then(|document| document["document_id"].as_str())
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let path = home
        .root()
        .join("voice-secretary")
        .join(group_id)
        .join("documents")
        .join(&document_id)
        .join("transcript.jsonl");
    Ok(Some((document_id, path)))
}

pub(super) fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
) -> io::Result<T> {
    migrate_legacy(home, group_id)?;
    with_lock(home, group_id, || {
        let index = load_index_unlocked(home, group_id)?;
        let mut flat = flat_from_index(&index);
        let state = flat.as_object_mut().expect("document state object");
        let result = change(state)?;
        let index = index_from_flat(group_id, state);
        save_index_unlocked(home, group_id, &index)?;
        Ok(result)
    })
}

pub(super) fn is_active(document: &Value) -> bool {
    document["status"]
        .as_str()
        .unwrap_or("active")
        .trim()
        .eq_ignore_ascii_case("active")
}

pub(super) fn is_deleted(document: &Value) -> bool {
    document["status"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("deleted")
}

pub(super) fn resolved_active<'a>(
    documents: &'a [Value],
    configured_id: &str,
    configured_path: &str,
) -> Option<&'a Value> {
    let configured_id = configured_id.trim();
    let configured_path = configured_path.trim();
    documents
        .iter()
        .find(|document| {
            is_active(document)
                && !configured_id.is_empty()
                && document["document_id"] == configured_id
        })
        .or_else(|| {
            documents.iter().find(|document| {
                is_active(document)
                    && !configured_path.is_empty()
                    && document["document_path"] == configured_path
            })
        })
        .or_else(|| {
            (!configured_id.is_empty() || !configured_path.is_empty())
                .then(|| latest_active(documents, None))
                .flatten()
        })
}

pub(super) fn latest_active<'a>(
    documents: &'a [Value],
    excluded_id: Option<&str>,
) -> Option<&'a Value> {
    documents
        .iter()
        .filter(|document| {
            is_active(document) && excluded_id.is_none_or(|id| document["document_id"] != id)
        })
        .max_by(|left, right| {
            updated_at(left).cmp(updated_at(right)).then_with(|| {
                left["document_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["document_id"].as_str().unwrap_or_default())
            })
        })
}

pub(super) fn active_path(state: &Value) -> Option<&str> {
    let documents = state["documents"].as_array()?;
    resolved_active(
        documents,
        state["active_document_id"].as_str().unwrap_or_default(),
        state["active_document_path"].as_str().unwrap_or_default(),
    )?["document_path"]
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

pub(super) fn needs_active_repair(state: &Value) -> bool {
    let configured_id = state["active_document_id"].as_str().unwrap_or_default();
    let configured_path = state["active_document_path"].as_str().unwrap_or_default();
    if configured_id.is_empty() && configured_path.is_empty() {
        return false;
    }
    let resolved = state["documents"]
        .as_array()
        .and_then(|documents| resolved_active(documents, configured_id, configured_path));
    resolved
        .and_then(|document| document["document_id"].as_str())
        .unwrap_or_default()
        != configured_id
        || resolved
            .and_then(|document| document["document_path"].as_str())
            .unwrap_or_default()
            != configured_path
}

pub(super) fn repair_active(state: &mut Map<String, Value>) {
    let configured_id = state
        .get("active_document_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let configured_path = state
        .get("active_document_path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if configured_id.is_empty() && configured_path.is_empty() {
        return;
    }
    let resolved = state
        .get("documents")
        .and_then(Value::as_array)
        .and_then(|documents| resolved_active(documents, configured_id, configured_path))
        .cloned();
    set_active(state, resolved.as_ref());
}

pub(super) fn set_active(state: &mut Map<String, Value>, document: Option<&Value>) {
    state.insert(
        "active_document_id".into(),
        document
            .map(|item| item["document_id"].clone())
            .unwrap_or_else(|| json!("")),
    );
    state.insert(
        "active_document_path".into(),
        document
            .map(|item| item["document_path"].clone())
            .unwrap_or_else(|| json!("")),
    );
}

fn updated_at(document: &Value) -> &str {
    document["updated_at"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| document["created_at"].as_str())
        .unwrap_or_default()
}

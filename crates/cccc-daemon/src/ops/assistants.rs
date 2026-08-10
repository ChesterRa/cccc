use cccc_contracts::{ActorRole, DaemonRequest, Event, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;

mod document_reconcile;
mod prompt_refine;
mod voice_ask;
mod voice_document_state;
mod voice_input;
mod voice_input_delivery;
mod voice_semantic_input;
mod voice_settings;

use crate::dispatch::{
    OpError, OpResult, bool_arg, first_non_blank_arg, object, required_arg, string_arg,
};
use crate::ops::actor_delivery;

const KEY: &str = "assistants";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "assistant_index" => document_reconcile::run(home, request)
            .and_then(|_| voice_settings::index(home, request)),
        "assistant_settings_update" => voice_settings::update(home, request),
        "assistant_status_update" => voice_settings::status(home, request),
        "assistant_voice_transcript_append" => voice_input::append(home, request),
        "assistant_voice_document_list" => documents(home, request),
        "assistant_voice_document_select" => select(home, request),
        "assistant_voice_document_input_read" => voice_input::read(home, request),
        "assistant_voice_document_save" => save(home, request),
        "assistant_voice_document_instruction" => voice_ask::input(home, request),
        "assistant_voice_document_archive" => archive(home, request),
        "assistant_voice_input_append" => prompt_refine::input(home, request),
        "assistant_voice_prompt_draft_submit" => prompt_refine::submit(home, request),
        "assistant_voice_prompt_draft_ack" => prompt_refine::ack(home, request),
        "assistant_voice_instruction_feedback" => voice_ask::feedback(home, request),
        "assistant_voice_ask_requests_clear" => voice_ask::clear(home, request),
        "assistant_voice_request" => voice_request(home, request),
        _ => return None,
    })
}

fn documents(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let value = document_reconcile::run(home, request)?;
    let requested_path = string_arg(request, "document_path").unwrap_or_default();
    let include_archived = bool_arg(request, "include_archived", false);
    let documents = items(&value, "documents")
        .iter()
        .filter(|document| {
            !voice_document_state::is_deleted(document)
                && (include_archived || voice_document_state::is_active(document))
                && (requested_path.is_empty() || document["document_path"] == requested_path)
        })
        .cloned()
        .collect::<Vec<_>>();
    object(
        json!({"group_id":group_id,"documents":documents,"active_document_id":value["active_document_id"],"active_document_path":value["active_document_path"]}),
    )
}
fn select(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    document_reconcile::run(home, request)?;
    let document = update(home, &group_id, |state| {
        let document = array(state, "documents")
            .iter()
            .find(|item| item["document_path"] == path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        if !voice_document_state::is_active(&document) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "voice document is not active",
            ));
        }
        state.insert("active_document_id".into(), document["document_id"].clone());
        state.insert("active_document_path".into(), json!(path));
        Ok(document)
    })?;
    document_result(home, request, &group_id, document, "selected")
}
fn save(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = string_arg(request, "document_path")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("voice/{}.md", short_id()));
    validate_path(&path)?;
    let title = string_arg(request, "title").unwrap_or_default();
    let content = string_arg(request, "content");
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let (storage_path, storage_kind) = document_storage_path(home, &group, &path)?;
    let mut previous_file = None::<Option<Vec<u8>>>;
    let result = update(home, &group_id, |state| {
        let docs = array(state, "documents");
        let index = docs.iter().position(|item| item["document_path"] == path);
        let is_new = index.is_none();
        let old = index
            .and_then(|index| docs.get(index))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let text = content
            .as_deref()
            .unwrap_or_else(|| old["content"].as_str().unwrap_or(""));
        let created_at = old["created_at"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(utc_now);
        let effective_title = if title.is_empty() {
            old["title"].as_str().unwrap_or("Untitled document")
        } else {
            &title
        };
        let changed = is_new
            || old["content"].as_str() != Some(text)
            || old["title"].as_str() != Some(effective_title);
        if let Some(text) = content.as_deref() {
            previous_file = Some(std::fs::read(&storage_path).ok());
            write_document(&storage_path, text)?;
        }
        let document = json!({"document_id":old["document_id"].as_str().map(str::to_owned).unwrap_or_else(||format!("vdoc_{}",short_id())),"document_path":path,"workspace_path":path,"absolute_path":storage_path,"filename":path.rsplit('/').next().unwrap_or(&path),"assistant_id":"voice_secretary","title":effective_title,"status":old["status"].as_str().unwrap_or("active"),"storage_kind":storage_kind,"content":text,"content_sha256":format!("{:x}",Sha256::digest(text.as_bytes())),"content_chars":text.chars().count(),"revision_count":old["revision_count"].as_u64().unwrap_or(0)+u64::from(changed),"created_at":created_at,"updated_at":utc_now(),"created_by":string_arg(request,"by").unwrap_or_else(||"user".into())});
        if let Some(index) = index {
            docs[index] = document.clone();
        } else {
            docs.push(document.clone());
        }
        state.insert("active_document_id".into(), document["document_id"].clone());
        state.insert("active_document_path".into(), json!(path));
        Ok(document)
    });
    let document = match result {
        Ok(document) => document,
        Err(error) => {
            if let Some(previous) = previous_file {
                let attempted_content = content.clone();
                if let Err(rollback_error) = store.mutate(&group_id, |group| {
                    let current_matches_attempt =
                        attempted_content.as_deref().is_some_and(|text| {
                            group.extra[KEY]["documents"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|document| {
                                    document["document_path"] == path
                                        && document["content"].as_str() == Some(text)
                                })
                        });
                    if !current_matches_attempt {
                        if let Some(bytes) = previous.as_deref() {
                            write_document_bytes(&storage_path, bytes)?;
                        } else if storage_path.exists() {
                            std::fs::remove_file(&storage_path)?;
                        }
                    }
                    Ok(())
                }) {
                    return Err(OpError::new(
                        "rollback_failed",
                        format!(
                            "{}; failed to reconcile voice document {path}: {rollback_error}",
                            error.message
                        ),
                    ));
                }
            }
            return Err(error);
        }
    };
    document_result(home, request, &group_id, document, "saved")
}

fn document_storage_path(
    home: &HomeLayout,
    group: &cccc_core::GroupDoc,
    relative: &str,
) -> Result<(std::path::PathBuf, &'static str), OpError> {
    validate_path(relative)?;
    if let Some(scope) = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
    {
        let root = std::path::Path::new(&scope.url)
            .canonicalize()
            .map_err(OpError::io)?;
        reject_symlink_components(&root, relative)?;
        return Ok((root.join(relative), "workspace"));
    }
    let root = home
        .root()
        .join("voice-secretary")
        .join(&group.group_id)
        .join("documents");
    std::fs::create_dir_all(&root).map_err(OpError::io)?;
    reject_symlink_components(&root, relative)?;
    Ok((root.join(relative), "rust_home"))
}

fn write_document(path: &std::path::Path, content: &str) -> io::Result<()> {
    write_document_bytes(path, content.as_bytes())
}
fn write_document_bytes(path: &std::path::Path, content: &[u8]) -> io::Result<()> {
    cccc_core::fs::atomic_write(path, content)
}
fn archive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    let document = update(home, &group_id, |state| {
        let document = {
            let item = array(state, "documents")
                .iter_mut()
                .find(|item| item["document_path"] == path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
            item["status"] = json!("archived");
            item["updated_at"] = json!(utc_now());
            item.clone()
        };
        let archived_id = document["document_id"].as_str().unwrap_or_default();
        let was_active =
            state["active_document_id"] == archived_id || state["active_document_path"] == path;
        if was_active {
            let next =
                voice_document_state::latest_active(array(state, "documents"), Some(archived_id))
                    .cloned();
            voice_document_state::set_active(state, next.as_ref());
        }
        Ok(document)
    })?;
    document_result(home, request, &group_id, document, "archived")
}
fn voice_request(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let text = first_non_blank_arg(request, &["text", "instruction", "request_text"])
        .ok_or_else(|| OpError::new("invalid_args", "text is required"))?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    let requested = string_arg(request, "target").unwrap_or_else(|| "@foreman".into());
    let target = if requested == "@foreman" {
        group
            .actors
            .iter()
            .find(|actor| {
                cccc_core::actors::effective_role(&group, &actor.id) == Some(ActorRole::Foreman)
            })
            .map(|actor| actor.id.clone())
            .ok_or_else(|| OpError::new("foreman_not_found", "group has no foreman actor"))?
    } else {
        requested
    };
    if target == "user" || target == "@all" || target == "voice-secretary" {
        return Err(OpError::new(
            "invalid_target",
            "Voice Secretary requests must target foreman or one concrete peer",
        ));
    }
    if !group.actors.iter().any(|actor| actor.id == target) {
        return Err(OpError::new(
            "actor_not_found",
            format!("actor not found: {target}"),
        ));
    }
    let item = add_request(
        home,
        &group_id,
        &text,
        &string_arg(request, "document_path").unwrap_or_default(),
        "peer_request",
    )?;
    let mut event = Event::new("system.notify", &group_id);
    event.by = "voice-secretary".into();
    event.data=json!({"kind":"voice_secretary_request","title":"Voice Secretary request","text":text,"to":[target],"priority":string_arg(request,"priority").unwrap_or_else(||"normal".into()),"requires_ack":request.args.get("requires_ack").and_then(Value::as_bool).unwrap_or(false),"context":{"kind":"voice_secretary_action_request","request":item}}).as_object().cloned().unwrap_or_default();
    cccc_core::ledger::append(&store.ledger_path(&group_id).map_err(OpError::io)?, &event)
        .map_err(OpError::io)?;
    let delivery = actor_delivery::dispatch(home, &group, &event);
    object(
        json!({"group_id":group_id,"assistant":voice_settings::effective_assistant(&load(home,&group_id)?),"request":item,"notify_event":event,"event":event,"delivery":delivery}),
    )
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
fn document_result(
    home: &HomeLayout,
    request: &DaemonRequest,
    group_id: &str,
    document: Value,
    action: &str,
) -> OpResult {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let mut event = Event::new("assistant.voice.document", group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = json!({"action":action,"assistant_id":"voice_secretary","document":document})
        .as_object()
        .cloned()
        .unwrap_or_default();
    cccc_core::ledger::append(&store.ledger_path(group_id).map_err(OpError::io)?, &event)
        .map_err(OpError::io)?;
    object(json!({"group_id":group_id,"document":document,"event":event}))
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
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        && path.extension().and_then(|value| value.to_str()) == Some("md"))
    .then_some(())
    .ok_or_else(|| {
        OpError::new(
            "invalid_args",
            "document_path must be a repository-relative Markdown path",
        )
    })
}
fn reject_symlink_components(root: &std::path::Path, relative: &str) -> Result<(), OpError> {
    let mut current = root.to_path_buf();
    for component in std::path::Path::new(relative).components() {
        let std::path::Component::Normal(name) = component else {
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
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(OpError::io(error)),
        }
    }
    Ok(())
}
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}

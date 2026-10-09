//! Bridge the global execution owner to existing Group-owned documents/requests.
use super::*;
use cccc_contracts::voice_secretary::{SecretaryTaskKind, SecretaryTaskPhase, SecretaryTaskTarget};
use cccc_core::voice_secretary::SecretaryTask;
use std::path::{Path, PathBuf};

pub(in crate::ops) fn target(
    home: &HomeLayout,
    group_id: &str,
    input: &Value,
) -> io::Result<SecretaryTaskTarget> {
    let group = GroupStore::new(home.clone())?.load(group_id)?;
    let mut path = input["document_path"].as_str().unwrap_or("").to_owned();
    let kind = if input["kind"] == "prompt_refine" {
        SecretaryTaskKind::Prompt
    } else if !path.is_empty() && input["metadata"]["task_kind"] != "ask" {
        SecretaryTaskKind::Document
    } else {
        SecretaryTaskKind::Ask
    };
    let mut scope_key = input["scope_key"]
        .as_str()
        .unwrap_or(&group.active_scope_key)
        .to_owned();
    let mut document_id = String::new();
    // A general Ask may read the current saved document as context only when it
    // belongs to the accepted scope. A stale/foreign reference never reroutes it.
    if kind == SecretaryTaskKind::Ask
        && path.is_empty()
        && let Some(reference) = input["trigger"]["current_document_path"]
            .as_str()
            .filter(|p| !p.is_empty())
    {
        let state = voice_document_state::load(home, group_id)?;
        let matches_scope = |doc: &Value| {
            if let Some(saved) = doc["scope_key"].as_str() {
                return saved == scope_key;
            }
            if group.scopes.is_empty() {
                return scope_key.is_empty();
            }
            doc["absolute_path"].as_str().is_some_and(|absolute| {
                group.scopes.iter().any(|scope| {
                    scope.scope_key == scope_key
                        && Path::new(&scope.url).join(reference) == Path::new(absolute)
                })
            })
        };
        if state["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|doc| {
                doc["document_path"] == reference
                    && voice_document_state::is_active(doc)
                    && matches_scope(doc)
            })
        {
            path = reference.into();
        }
    }
    if !path.is_empty() {
        validate_path(&path).map_err(io_error)?;
        let state = voice_document_state::load(home, group_id)?;
        let doc = state["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|doc| doc["document_path"] == path && voice_document_state::is_active(doc))
            .ok_or_else(|| io::Error::other("Secretary target document is missing or archived"))?;
        document_id = doc["document_id"].as_str().unwrap_or("").to_owned();
        if let Some(saved_scope) = doc["scope_key"].as_str() {
            scope_key = saved_scope.into();
        } else if let Some(absolute) = doc["absolute_path"].as_str() {
            if let Some(scope) = group
                .scopes
                .iter()
                .find(|scope| Path::new(&scope.url).join(&path) == Path::new(absolute))
            {
                scope_key = scope.scope_key.clone();
            }
        }
    }
    if !scope_key.is_empty() && !group.scopes.iter().any(|s| s.scope_key == scope_key) {
        return Err(io::Error::other(
            "Secretary target scope is no longer attached",
        ));
    }
    Ok(SecretaryTaskTarget {
        group_id: group_id.into(),
        scope_key,
        kind,
        document_id,
        document_path: path,
        request_id: input["request_id"].as_str().unwrap_or("").into(),
        composer_snapshot_hash: input["composer_snapshot_hash"]
            .as_str()
            .unwrap_or("")
            .into(),
    })
}

pub(in crate::ops) fn guidance(home: &HomeLayout, group_id: &str) -> io::Result<String> {
    GroupStore::new(home.clone())?.load(group_id)?;
    cccc_core::voice_secretary_settings::guidance(home)
}

pub(in crate::ops) fn document_file(
    home: &HomeLayout,
    task: &SecretaryTask,
) -> io::Result<PathBuf> {
    let group = GroupStore::new(home.clone())?.load(&task.target.group_id)?;
    validate_path(&task.target.document_path).map_err(io_error)?;
    let root = if task.target.scope_key.is_empty() {
        if !group.scopes.is_empty() {
            return Err(io::Error::other("Task has no workspace scope"));
        }
        home.root()
            .join("voice-secretary")
            .join(&task.target.group_id)
            .join("documents")
    } else {
        let scope = group
            .scopes
            .iter()
            .find(|s| s.scope_key == task.target.scope_key)
            .ok_or_else(|| io::Error::other("Secretary target scope is no longer attached"))?;
        PathBuf::from(&scope.url)
    };
    let root = root.canonicalize()?;
    reject_symlink_components(&root, &task.target.document_path).map_err(io_error)?;
    let file = root.join(&task.target.document_path);
    let state = voice_document_state::load(home, &task.target.group_id)?;
    let doc = state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|doc| {
            doc["document_id"] == task.target.document_id
                && doc["document_path"] == task.target.document_path
                && voice_document_state::is_active(doc)
        })
        .ok_or_else(|| io::Error::other("Secretary document was moved, archived or removed"))?;
    if let Some(absolute) = doc["absolute_path"].as_str()
        && Path::new(absolute).canonicalize()? != file.canonicalize()?
    {
        return Err(io::Error::other(
            "Secretary document belongs to another workspace target",
        ));
    }
    Ok(file)
}

pub(in crate::ops) fn validate_document(home: &HomeLayout, task: &SecretaryTask) -> io::Result<()> {
    if task.target.document_id.is_empty() {
        return Ok(());
    }
    let file = document_file(home, task)?.canonicalize()?;
    if task
        .document_file
        .as_ref()
        .is_some_and(|original| *original != file)
    {
        return Err(io::Error::other(
            "Secretary document target changed after acceptance",
        ));
    }
    Ok(())
}

pub(in crate::ops) fn validate_request(home: &HomeLayout, task: &SecretaryTask) -> io::Result<()> {
    let group = GroupStore::new(home.clone())?.load(&task.target.group_id)?;
    if !task.target.scope_key.is_empty()
        && !group
            .scopes
            .iter()
            .any(|scope| scope.scope_key == task.target.scope_key)
    {
        return Err(io::Error::other(
            "Secretary target scope is no longer attached",
        ));
    }
    validate_document(home, task)?;
    if task.target.request_id.is_empty() {
        return Ok(());
    }
    let state = assistant_state::load(home, &task.target.group_id)?;
    let input = task
        .inputs
        .last()
        .ok_or_else(|| io::Error::other("Task source is missing"))?;
    if task.target.kind == SecretaryTaskKind::Prompt {
        let current = &state["voice_prompt_requests"][&task.target.request_id];
        if !current.is_object()
            || current["composer_snapshot_hash"].as_str().unwrap_or("")
                != task.target.composer_snapshot_hash
            || current["last_input_append_id"] != input["input_append_id"]
        {
            return Err(io::Error::other(
                "Composer request changed; secretary candidate must not replace it",
            ));
        }
    } else if task.target.kind == SecretaryTaskKind::Ask {
        let current = state["ask_requests"]
            .as_array()
            .into_iter()
            .flatten()
            // Clearing history hides it; it does not cancel accepted work.
            .find(|item| item["request_id"] == task.target.request_id)
            .ok_or_else(|| io::Error::other("Ask request was removed"))?;
        if current["input_append_id"] != input["input_append_id"] {
            return Err(io::Error::other(
                "Ask request changed after task acceptance",
            ));
        }
    }
    Ok(())
}

pub(in crate::ops) fn project_result(home: &HomeLayout, task: &SecretaryTask) -> io::Result<()> {
    let fallback;
    let receipt = if let Some(receipt) = task.receipt.as_ref() {
        receipt
    } else {
        if task.phase.executing() || task.phase == SecretaryTaskPhase::Queued {
            return Ok(());
        }
        fallback = cccc_core::voice_secretary::SecretaryReceipt {
            status: task.phase,
            output: json!({"reply_text":if task.diagnostic.is_empty() { format!("Secretary task {:?}; original input retained.",task.phase) } else { task.diagnostic.clone() }}),
            document_version: String::new(),
            committed_at: task.updated_at.clone(),
        };
        &fallback
    };
    if !task.target.request_id.is_empty() {
        validate_request(home, task)?;
    }
    let mut args = receipt.output.as_object().cloned().unwrap_or_default();
    args.insert("group_id".into(), json!(task.target.group_id));
    args.insert("request_id".into(), json!(task.target.request_id));
    args.insert("by".into(), json!("assistant:voice_secretary"));
    args.insert("scope_key".into(), json!(task.target.scope_key));
    args.insert("secretary_task_id".into(), json!(task.task_id));
    args.insert(
        "status".into(),
        json!(match receipt.status {
            SecretaryTaskPhase::Done => "done",
            SecretaryTaskPhase::NeedsUser => "needs_user",
            _ => "failed",
        }),
    );
    let request = DaemonRequest {
        v: 1,
        op: String::new(),
        args,
    };
    if task.target.kind == SecretaryTaskKind::Prompt && receipt.status == SecretaryTaskPhase::Done {
        let state = assistant_state::load(home, &task.target.group_id)?;
        let current = &state["voice_prompt_requests"][&task.target.request_id];
        if current["composer_snapshot_hash"].as_str().unwrap_or("")
            != task.target.composer_snapshot_hash
        {
            return Err(io::Error::other(
                "Composer request changed; secretary draft was retained for review",
            ));
        }
        prompt_refine::submit(home, &request).map_err(io_error)?;
    } else if task.target.kind == SecretaryTaskKind::Prompt {
        assistant_state::update(home, &task.target.group_id, |state| {
            state["voice_prompt_requests"][&task.target.request_id]["secretary_status"] =
                json!(task.phase);
            state["voice_prompt_requests"][&task.target.request_id]["secretary_message"] =
                receipt.output["reply_text"].clone();
            Ok(())
        })?;
    } else if !task.target.request_id.is_empty() {
        voice_ask::feedback(home, &request).map_err(io_error)?;
    }
    if task.target.kind == SecretaryTaskKind::Document && receipt.status == SecretaryTaskPhase::Done
    {
        let file = document_file(home, task)?;
        let bytes = std::fs::read(&file)?;
        // Do not replace a later user's content projection with the old submission.
        if cccc_core::voice_secretary::digest(&bytes) == receipt.document_version {
            let content = String::from_utf8(bytes).map_err(io::Error::other)?;
            voice_document_state::update(home, &task.target.group_id, |state| {
                if let Some(doc) = array(state, "documents")
                    .iter_mut()
                    .find(|doc| doc["document_id"] == task.target.document_id)
                {
                    if doc["last_secretary_task_id"] != task.task_id {
                        let changed = doc["content_sha256"] != receipt.document_version;
                        doc["content"] = json!(content);
                        doc["content_sha256"] = json!(receipt.document_version);
                        doc["content_chars"] = json!(content.chars().count());
                        if changed {
                            doc["revision_count"] =
                                json!(doc["revision_count"].as_u64().unwrap_or(0) + 1);
                        }
                        doc["last_secretary_task_id"] = json!(task.task_id);
                        doc["updated_at"] = json!(receipt.committed_at);
                    }
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn io_error(error: OpError) -> io::Error {
    io::Error::other(format!("{}: {}", error.code, error.message))
}

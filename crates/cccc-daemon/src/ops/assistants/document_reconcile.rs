use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, ledger};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::dispatch::{OpError, required_arg, string_arg};

use super::{
    array, document_storage_path, reject_symlink_components, voice_document_state, voice_settings,
};

const MAX_DISCOVERED_DOCUMENTS: usize = 100;

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> Result<Value, OpError> {
    let group_id = required_arg(request, "group_id")?;
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let mut state = voice_document_state::load(home, &group_id).map_err(OpError::io)?;
    let discovered = discover_workspace_documents(&group, &state)?;
    if !discovered.is_empty() {
        state = voice_document_state::update(home, &group_id, |state| {
            let documents = array(state, "documents");
            for document in discovered {
                let path = document["document_path"].as_str().unwrap_or_default();
                if !documents
                    .iter()
                    .any(|existing| existing["document_path"] == path)
                {
                    documents.push(document);
                }
            }
            Ok(Value::Object(state.clone()))
        })
        .map_err(OpError::io)?;
    }
    let targets = state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|document| document["document_path"].as_str())
        .map(|path| {
            let (storage_path, storage_kind) = document_storage_path(home, &group, path)?;
            Ok((path.to_owned(), storage_path, storage_kind))
        })
        .collect::<Result<Vec<_>, OpError>>()?;
    let needs_content_reconcile = targets
        .iter()
        .try_fold(
            false,
            |changed, (path, storage_path, _)| -> io::Result<bool> {
                let content = match std::fs::read_to_string(storage_path) {
                    Ok(content) => content,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(changed),
                    Err(error) => return Err(error),
                };
                let stored = state["documents"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|document| document["document_path"] == *path)
                    .and_then(|document| document["content"].as_str());
                Ok(changed || stored != Some(content.as_str()))
            },
        )
        .map_err(OpError::io)?;
    let needs_reconcile =
        needs_content_reconcile || voice_document_state::needs_active_repair(&state);
    if !needs_reconcile {
        return Ok(state);
    }

    let (state, changed) = voice_document_state::update(home, &group_id, |state| {
        let mut changed = Vec::new();
        for (path, storage_path, storage_kind) in &targets {
            let content = match std::fs::read_to_string(storage_path) {
                Ok(content) => content,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let Some(document) = array(state, "documents")
                .iter_mut()
                .find(|document| document["document_path"] == *path)
            else {
                continue;
            };
            if document["content"].as_str() == Some(content.as_str()) {
                continue;
            }
            document["content"] = json!(content);
            document["content_sha256"] = json!(format!("{:x}", Sha256::digest(content.as_bytes())));
            document["content_chars"] = json!(content.chars().count());
            document["revision_count"] =
                json!(document["revision_count"].as_u64().unwrap_or(0) + 1);
            document["updated_at"] = json!(utc_now());
            document["absolute_path"] = json!(storage_path);
            document["storage_kind"] = json!(storage_kind);
            changed.push(document.clone());
        }
        voice_document_state::repair_active(state);
        Ok((Value::Object(state.clone()), changed))
    })
    .map_err(OpError::io)?;

    if !changed.is_empty() {
        let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
        let ledger_path = store.ledger_path(&group_id).map_err(OpError::io)?;
        let by = string_arg(request, "by").unwrap_or_else(|| "system".into());
        for document in changed {
            let mut event = Event::new("assistant.voice.document", &group_id);
            event.by = by.clone();
            event.data =
                json!({"action":"reconciled","assistant_id":"voice_secretary","document":document})
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
            ledger::append(&ledger_path, &event).map_err(OpError::io)?;
        }
    }

    Ok(state)
}

fn discover_workspace_documents(
    group: &cccc_core::GroupDoc,
    state: &Value,
) -> Result<Vec<Value>, OpError> {
    let Some(scope) = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
    else {
        return Ok(Vec::new());
    };
    let root = Path::new(&scope.url).canonicalize().map_err(OpError::io)?;
    let assistant_state = group.extra.get("assistants").unwrap_or(&Value::Null);
    let assistant = voice_settings::effective_assistant(assistant_state);
    let configured = assistant["config"]["document_default_dir"]
        .as_str()
        .unwrap_or("docs/voice-secretary")
        .trim()
        .replace('\\', "/");
    if configured.starts_with('/') || configured.split('/').any(|part| part == "..") {
        return Err(OpError::new(
            "invalid_args",
            "document_default_dir must stay under the group's active scope root",
        ));
    }
    let configured = if configured.is_empty() {
        "docs/voice-secretary"
    } else {
        configured.trim_matches('/')
    };
    let relative_dir = configured
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .fold(PathBuf::new(), |path, part| path.join(part));
    let directory = root.join(&relative_dir);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    reject_symlink_components(
        &root,
        portable_path(&relative_dir).as_deref().unwrap_or_default(),
    )?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let directory = directory.canonicalize().map_err(OpError::io)?;
    if !directory.starts_with(&root) {
        return Err(OpError::new(
            "invalid_args",
            "document_default_dir must stay under the group's active scope root",
        ));
    }

    let mut candidates = Vec::new();
    collect_markdown_candidates(&directory, Path::new(""), &mut candidates).map_err(OpError::io)?;
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let existing = state["documents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|document| document["document_path"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut documents = Vec::new();
    for (modified, relative, path) in candidates.into_iter().take(MAX_DISCOVERED_DOCUMENTS) {
        let Some(workspace_path) = portable_path(&relative_dir.join(&relative)) else {
            continue;
        };
        if existing.contains(workspace_path.as_str()) {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(OpError::io)?;
        let fallback = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled document");
        let title = title_from_markdown(&content, fallback);
        let updated_at: DateTime<Utc> = modified.into();
        let document_id = format!("voice-doc-{:x}", Sha1::digest(workspace_path.as_bytes()));
        documents.push(json!({
            "schema":1,
            "document_id":document_id.chars().take(34).collect::<String>(),
            "assistant_id":"voice_secretary",
            "title":title,
            "status":"active",
            "storage_kind":"workspace",
            "workspace_path":workspace_path,
            "document_path":workspace_path,
            "absolute_path":path,
            "filename":path.file_name().and_then(|value|value.to_str()).unwrap_or_default(),
            "content":content,
            "content_sha256":format!("{:x}",Sha256::digest(content.as_bytes())),
            "content_chars":content.chars().count(),
            "created_at":updated_at.to_rfc3339(),
            "updated_at":updated_at.to_rfc3339(),
            "created_by":"workspace_import",
            "revision_count":0,
            "source_segment_count":0,
            "last_source_segment_id":"",
            "discovered":true
        }));
    }
    Ok(documents)
}

fn collect_markdown_candidates(
    directory: &Path,
    relative: &Path,
    candidates: &mut Vec<(SystemTime, PathBuf, PathBuf)>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let child_relative = relative.join(entry.file_name());
        if file_type.is_dir() {
            if entry.file_name() != "archive" {
                collect_markdown_candidates(&entry.path(), &child_relative, candidates)?;
            }
            continue;
        }
        if !file_type.is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, child_relative, entry.path()));
    }
    Ok(())
}

fn portable_path(path: &Path) -> Option<String> {
    path.components()
        .map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => Some(""),
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| {
            parts
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        })
}

fn title_from_markdown(content: &str, fallback: &str) -> String {
    let mut lines = content.lines().take(120);
    if lines.next().is_some_and(|line| line.trim() == "---") {
        for line in &mut lines {
            let line = line.trim();
            if matches!(line, "---" | "...") {
                break;
            }
            if let Some(title) = line
                .strip_prefix("title:")
                .map(str::trim)
                .map(|value| value.trim_matches(['\'', '"']).trim())
                .filter(|value| !value.is_empty())
            {
                return title.chars().take(160).collect();
            }
        }
    }
    content
        .lines()
        .take(120)
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(160).collect())
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[cfg(unix)]
    #[test]
    fn unreadable_document_is_reported_instead_of_treated_as_unchanged() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("documents", "").expect("group");
        let relative = "notes/reconcile.md";
        let path = home
            .root()
            .join("voice-secretary")
            .join(&group.group_id)
            .join("documents")
            .join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("document dir");
        std::fs::write(&path, "disk content").expect("document");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("make unreadable");
        cccc_core::assistant_state::update(&home, &group.group_id, |state| {
            state.insert(
                "documents".into(),
                json!([{
                    "document_path":relative,
                    "content":"stored content",
                    "revision_count":1
                }]),
            );
            Ok(())
        })
        .expect("assistant state");
        let request = DaemonRequest {
            v: 1,
            op: "assistant_index".into(),
            args: Map::from_iter([("group_id".into(), json!(group.group_id))]),
        };

        let error = run(&home, &request).expect_err("unreadable file must fail");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("restore permissions");
        assert!(!error.message.is_empty());
    }
}

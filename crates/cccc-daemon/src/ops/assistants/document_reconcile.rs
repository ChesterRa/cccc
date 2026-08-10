use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;

use crate::dispatch::{OpError, required_arg, string_arg};

use super::{array, document_storage_path, load, update, voice_document_state};

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> Result<Value, OpError> {
    let group_id = required_arg(request, "group_id")?;
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let state = load(home, &group_id)?;
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

    let (state, changed) = update(home, &group_id, |state| {
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
    })?;

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
        cccc_core::integration_state::group_update(
            &store,
            &group.group_id,
            "assistants",
            |value| {
                *value = json!({
                    "documents":[{
                        "document_path":relative,
                        "content":"stored content",
                        "revision_count":1
                    }]
                });
                Ok(())
            },
        )
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

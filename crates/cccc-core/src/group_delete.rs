use serde_json::{Map, Value, json};
use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

use crate::{GroupStore, Registry, web_model_connectors};

#[derive(Default)]
struct RetiredGroupSpace {
    binding: Option<Value>,
    jobs: Map<String, Value>,
    payload_refs: Vec<String>,
}

fn space_path(store: &GroupStore, name: &str) -> std::path::PathBuf {
    store.home().root().join("state/space").join(name)
}

fn read_json_doc(path: &Path) -> io::Result<Value> {
    if path.exists() {
        crate::fs::read_json(path)
    } else {
        Ok(json!({}))
    }
}

fn object_field<'a>(doc: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !doc.is_object() {
        *doc = json!({});
    }
    let root = doc.as_object_mut().expect("JSON object initialized");
    let field = root.entry(key).or_insert_with(|| json!({}));
    if !field.is_object() {
        *field = json!({});
    }
    field
        .as_object_mut()
        .expect("JSON object field initialized")
}

fn retire_group_space(store: &GroupStore, group_id: &str) -> io::Result<RetiredGroupSpace> {
    let bindings_path = space_path(store, "bindings.json");
    let binding =
        crate::fs::with_exclusive_lock(&bindings_path.with_extension("json.lock"), || {
            let mut doc = read_json_doc(&bindings_path)?;
            let removed = object_field(&mut doc, "bindings").remove(group_id);
            if removed.is_some() {
                doc["updated_at"] = json!(cccc_contracts::utc_now());
                crate::fs::write_json(&bindings_path, &doc)?;
            }
            Ok(removed)
        })?;

    let jobs_path = space_path(store, "jobs.json");
    let jobs_result =
        crate::fs::with_exclusive_lock(&jobs_path.with_extension("json.lock"), || {
            let mut doc = read_json_doc(&jobs_path)?;
            let jobs = object_field(&mut doc, "jobs");
            let ids = jobs
                .iter()
                .filter(|(_, job)| job["group_id"].as_str() == Some(group_id))
                .map(|(job_id, _)| job_id.clone())
                .collect::<Vec<_>>();
            let mut removed = Map::new();
            let mut payload_refs = Vec::new();
            for job_id in ids {
                if let Some(job) = jobs.remove(&job_id) {
                    if let Some(payload_ref) = job["payload_ref"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                    {
                        payload_refs.push(payload_ref.to_owned());
                    }
                    removed.insert(job_id, job);
                }
            }
            if !removed.is_empty() {
                doc["updated_at"] = json!(cccc_contracts::utc_now());
                crate::fs::write_json(&jobs_path, &doc)?;
            }
            Ok((removed, payload_refs))
        });
    match jobs_result {
        Ok((jobs, payload_refs)) => Ok(RetiredGroupSpace {
            binding,
            jobs,
            payload_refs,
        }),
        Err(error) => {
            let retired = RetiredGroupSpace {
                binding,
                ..RetiredGroupSpace::default()
            };
            match restore_group_space(store, group_id, &retired) {
                Ok(()) => Err(error),
                Err(rollback) => Err(io::Error::other(format!(
                    "{error}; rollback_failed: could not restore NotebookLM binding: {rollback}"
                ))),
            }
        }
    }
}

fn restore_group_space(
    store: &GroupStore,
    group_id: &str,
    retired: &RetiredGroupSpace,
) -> io::Result<()> {
    if let Some(binding) = retired.binding.as_ref() {
        let path = space_path(store, "bindings.json");
        crate::fs::with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut doc = read_json_doc(&path)?;
            object_field(&mut doc, "bindings").insert(group_id.into(), binding.clone());
            doc["updated_at"] = json!(cccc_contracts::utc_now());
            crate::fs::write_json(&path, &doc)
        })?;
    }
    if !retired.jobs.is_empty() {
        let path = space_path(store, "jobs.json");
        crate::fs::with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut doc = read_json_doc(&path)?;
            object_field(&mut doc, "jobs").extend(retired.jobs.clone());
            doc["updated_at"] = json!(cccc_contracts::utc_now());
            crate::fs::write_json(&path, &doc)
        })?;
    }
    Ok(())
}

fn finalize_group_space(store: &GroupStore, retired: &RetiredGroupSpace) {
    let root = store.home().root().join("state/space/job_payloads");
    for payload_ref in &retired.payload_refs {
        let path = Path::new(payload_ref);
        if path.components().count() == 1 && path.file_name().is_some() {
            let _ = fs::remove_file(root.join(path));
        }
    }
}

pub(crate) fn delete(store: &GroupStore, group_id: &str) -> io::Result<bool> {
    delete_with(store, group_id, |path| fs::remove_dir_all(path))
}

fn delete_with(
    store: &GroupStore,
    group_id: &str,
    remove_tombstone: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    delete_with_steps(store, group_id, remove_tombstone, unregister)
}

fn delete_with_steps(
    store: &GroupStore,
    group_id: &str,
    remove_tombstone: impl FnOnce(&Path) -> io::Result<()>,
    unregister_group: impl Fn(&GroupStore, &str) -> io::Result<()>,
) -> io::Result<bool> {
    let dir = store.group_dir(group_id)?;
    if !dir.exists() {
        let retired_connectors = web_model_connectors::retire_group(store.home(), group_id)?;
        let retired_space = match retire_group_space(store, group_id) {
            Ok(retired) => retired,
            Err(error) => {
                return Err(restore_connectors_after_failed_delete(
                    store,
                    &retired_connectors,
                    error,
                ));
            }
        };
        if let Err(error) = unregister_group(store, group_id) {
            return Err(restore_state_after_failed_delete(
                store,
                group_id,
                &retired_connectors,
                &retired_space,
                error,
            ));
        }
        finalize_group_space(store, &retired_space);
        cleanup_tombstones(store, group_id)?;
        return Ok(false);
    }
    let ledger_path = dir.join("ledger.jsonl");
    let tombstone = store
        .home()
        .groups_dir()
        .join(format!(".{group_id}.deleting-{}", Uuid::new_v4().simple()));
    fs::rename(&dir, &tombstone)?;
    let retired_connectors = match web_model_connectors::retire_group(store.home(), group_id) {
        Ok(retired) => retired,
        Err(error) => {
            return Err(match fs::rename(&tombstone, &dir) {
                Ok(()) => error,
                Err(rollback) => io::Error::other(format!(
                    "{error}; rollback_failed: could not restore {}: {rollback}",
                    dir.display()
                )),
            });
        }
    };
    let retired_space = match retire_group_space(store, group_id) {
        Ok(retired) => retired,
        Err(error) => {
            let error = restore_connectors_after_failed_delete(store, &retired_connectors, error);
            return Err(match fs::rename(&tombstone, &dir) {
                Ok(()) => error,
                Err(rollback) => io::Error::other(format!(
                    "{error}; rollback_failed: could not restore {}: {rollback}",
                    dir.display()
                )),
            });
        }
    };
    if let Err(error) = unregister_group(store, group_id) {
        let error = match fs::rename(&tombstone, &dir) {
            Ok(()) => error,
            Err(rollback) => io::Error::other(format!(
                "{error}; rollback_failed: could not restore {}: {rollback}",
                dir.display()
            )),
        };
        return Err(restore_state_after_failed_delete(
            store,
            group_id,
            &retired_connectors,
            &retired_space,
            error,
        ));
    }
    crate::ledger_index::invalidate_path(&ledger_path);
    let cleanup = remove_tombstone(&tombstone).map_err(|error| {
        io::Error::other(format!(
            "rollback_failed: group was unregistered but tombstone {} could not be removed: {error}",
            tombstone.display()
        ))
    });
    finalize_group_space(store, &retired_space);
    cleanup?;
    Ok(true)
}

fn restore_connectors_after_failed_delete(
    store: &GroupStore,
    retired: &[serde_json::Value],
    original: io::Error,
) -> io::Error {
    match web_model_connectors::restore(store.home(), retired) {
        Ok(()) => original,
        Err(rollback) => io::Error::other(format!(
            "{original}; rollback_failed: could not restore web-model connectors: {rollback}"
        )),
    }
}

fn restore_state_after_failed_delete(
    store: &GroupStore,
    group_id: &str,
    retired_connectors: &[Value],
    retired_space: &RetiredGroupSpace,
    original: io::Error,
) -> io::Error {
    let mut failures = Vec::new();
    if let Err(error) = restore_group_space(store, group_id, retired_space) {
        failures.push(format!("could not restore NotebookLM state: {error}"));
    }
    if let Err(error) = web_model_connectors::restore(store.home(), retired_connectors) {
        failures.push(format!("could not restore web-model connectors: {error}"));
    }
    if failures.is_empty() {
        original
    } else {
        io::Error::other(format!(
            "{original}; rollback_failed: {}",
            failures.join("; ")
        ))
    }
}

fn unregister(store: &GroupStore, group_id: &str) -> io::Result<()> {
    Registry::mutate(store.home(), |registry| {
        registry.groups.remove(group_id);
        registry.defaults.retain(|_, value| value != group_id);
        Ok(())
    })
}

fn cleanup_tombstones(store: &GroupStore, group_id: &str) -> io::Result<()> {
    let prefix = format!(".{group_id}.deleting-");
    for entry in fs::read_dir(store.home().groups_dir())? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HomeLayout;

    #[test]
    fn failed_tombstone_cleanup_is_explicit_and_retryable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("cleanup", "").expect("group");

        let error = delete_with(&store, &group.group_id, |_| {
            Err(io::Error::other("injected cleanup failure"))
        })
        .expect_err("cleanup must fail explicitly");
        assert!(error.to_string().contains("rollback_failed"));
        assert!(!store.group_dir(&group.group_id).expect("dir").exists());
        assert!(
            !Registry::load(&home)
                .expect("registry")
                .groups
                .contains_key(&group.group_id)
        );
        assert!(has_tombstone(&home, &group.group_id));

        assert!(!delete(&store, &group.group_id).expect("retry cleanup"));
        assert!(!has_tombstone(&home, &group.group_id));
    }

    #[test]
    fn deleting_a_group_invalidates_its_original_ledger_index_key() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("cached", "").expect("group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        crate::ledger::append(
            &ledger_path,
            &cccc_contracts::Event::new("chat.message", &group.group_id),
        )
        .expect("append event");
        crate::ledger::read_all(&ledger_path).expect("populate index");
        assert!(crate::ledger_index::is_cached(&ledger_path));

        assert!(delete(&store, &group.group_id).expect("delete group"));

        assert!(
            !crate::ledger_index::is_cached(&ledger_path),
            "the cache key is the pre-rename ledger path"
        );
    }
    #[test]
    fn retry_keeps_the_only_tombstone_when_unregister_still_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("recoverable", "").expect("group");
        let directory = store.group_dir(&group.group_id).expect("group directory");
        let tombstone = home
            .groups_dir()
            .join(format!(".{}.deleting-recovery", group.group_id));
        std::fs::rename(&directory, &tombstone).expect("simulate failed rollback");

        let error = delete_with_steps(
            &store,
            &group.group_id,
            |path| std::fs::remove_dir_all(path),
            |_, _| Err(io::Error::other("injected registry failure")),
        )
        .expect_err("unregister failure");

        assert!(error.to_string().contains("injected registry failure"));
        assert!(tombstone.is_dir(), "the only data copy must remain");
        assert!(
            Registry::load(&home)
                .expect("registry")
                .groups
                .contains_key(&group.group_id)
        );
    }

    fn has_tombstone(home: &HomeLayout, group_id: &str) -> bool {
        home.groups_dir().read_dir().expect("entries").any(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(&format!(".{group_id}.deleting-"))
        })
    }
}

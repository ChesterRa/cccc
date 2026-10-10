use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

use super::model::ContextDoc;
use super::yaml_storage::{self, ContextPaths};
use crate::fs::{read_json, write_json};

pub(super) fn migrate_legacy_json(paths: &ContextPaths) -> io::Result<()> {
    if paths.migration_file.is_file() || !paths.legacy_file.is_file() {
        return Ok(());
    }
    let legacy = match read_json::<ContextDoc>(&paths.legacy_file) {
        Ok(document) => document,
        Err(_) => return Ok(()),
    };
    let before = yaml_storage::load(paths)?;
    let (mut merged, mappings) = merge(before.clone(), legacy)?;
    yaml_storage::touch_updated_at(&mut merged);
    yaml_storage::persist_diff(paths, &before, &merged)?;
    write_json(
        &paths.migration_file,
        &json!({
            "v":1,
            "migrated_at":utc_now(),
            "source":"state/context.json",
            "task_id_mappings":mappings,
        }),
    )
}

fn merge(
    mut canonical: ContextDoc,
    legacy: ContextDoc,
) -> io::Result<(ContextDoc, BTreeMap<String, String>)> {
    merge_coordination(&mut canonical.coordination, legacy.coordination);
    merge_missing(&mut canonical.meta, legacy.meta);
    for (actor_id, state) in legacy.agent_states {
        canonical.agent_states.entry(actor_id).or_insert(state);
    }

    let mut next_number = canonical
        .tasks
        .iter()
        .filter_map(task_id)
        .filter_map(|id| id.strip_prefix('T'))
        .filter_map(|number| number.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        .max(canonical.task_id_high_water);
    let canonical_ids = canonical
        .tasks
        .iter()
        .filter_map(task_id)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut reserved = canonical_ids.clone();
    let mut mappings = BTreeMap::new();
    for task in &legacy.tasks {
        let old_id = task_id(task).unwrap_or_default();
        if let Some(existing) = equivalent_task_id(&canonical.tasks, task) {
            if !old_id.is_empty() {
                mappings.insert(old_id.to_owned(), existing);
            }
            continue;
        }
        let available_number = old_id
            .strip_prefix('T')
            .and_then(|number| number.parse::<u64>().ok())
            .is_some_and(|number| number > canonical.task_id_high_water);
        let new_id = if yaml_storage::is_canonical_task_id(old_id)
            && available_number
            && !reserved.contains(old_id)
        {
            old_id.to_owned()
        } else {
            loop {
                next_number = next_number.checked_add(1).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "task number exhausted")
                })?;
                let candidate = format!("T{next_number:03}");
                if !reserved.contains(&candidate) {
                    break candidate;
                }
            }
        };
        reserved.insert(new_id.clone());
        if !old_id.is_empty() {
            mappings.insert(old_id.to_owned(), new_id);
        }
    }
    for mut task in legacy.tasks {
        let old_id = task_id(&task).unwrap_or_default().to_owned();
        let Some(new_id) = mappings.get(&old_id).cloned() else {
            continue;
        };
        if let Some(existing) = canonical
            .tasks
            .iter_mut()
            .find(|existing| task_id(existing) == Some(new_id.as_str()))
        {
            restore_equivalent_references(
                existing,
                &task,
                &mappings,
                &canonical_ids,
                &reserved,
                canonical.task_id_high_water,
            )?;
            continue;
        }
        task.insert("id".into(), Value::String(new_id));
        rewrite_task_references(&mut task, &mappings);
        canonical.tasks.push(task);
    }
    canonical.task_id_high_water = canonical
        .task_id_high_water
        .max(super::task_apply::maximum_task_number(&canonical.tasks));
    Ok((canonical, mappings))
}

fn merge_coordination(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for key in ["brief", "recent_decisions", "recent_handoffs"] {
        let Some(value) = source.get(key) else {
            continue;
        };
        match (target.get_mut(key), value) {
            (Some(Value::Object(current)), Value::Object(incoming)) => {
                merge_missing(current, incoming.clone());
            }
            (Some(Value::Array(current)), Value::Array(incoming)) => {
                for item in incoming {
                    if !current.contains(item) {
                        current.push(item.clone());
                    }
                }
            }
            (None, value) => {
                target.insert(key.into(), value.clone());
            }
            _ => {}
        }
    }
    if let Some(Value::Array(notes)) = source.get("notes") {
        for note in notes {
            let key = match note.get("kind").and_then(Value::as_str) {
                Some("handoff") => "recent_handoffs",
                _ => "recent_decisions",
            };
            let items = target
                .entry(key)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut();
            let Some(items) = items else {
                continue;
            };
            let converted = json!({
                "at":note.get("created_at"),
                "by":note.get("by"),
                "summary":note.get("summary"),
                "task_id":note.get("task_id"),
            });
            if !items.contains(&converted) {
                items.push(converted);
            }
        }
    }
}

fn merge_missing(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        let missing = target
            .get(&key)
            .is_none_or(|current| current.is_null() || current.as_str() == Some(""));
        if missing {
            target.insert(key, value);
        }
    }
}

fn equivalent_task_id(
    tasks: &[Map<String, Value>],
    candidate: &Map<String, Value>,
) -> Option<String> {
    let client_id = candidate
        .get("client_request_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let title = candidate.get("title").and_then(Value::as_str);
    tasks.iter().find_map(|task| {
        let existing_client = task
            .get("client_request_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let same_client = client_id.is_some() && client_id == existing_client;
        let conflicting_clients =
            client_id.is_some() && existing_client.is_some() && client_id != existing_client;
        let same_title = title.is_some_and(|value| {
            !value.is_empty() && task.get("title").and_then(Value::as_str) == Some(value)
        });
        (same_client || (same_title && !conflicting_clients))
            .then(|| task_id(task).map(str::to_owned))
            .flatten()
    })
}

fn restore_equivalent_references(
    existing: &mut Map<String, Value>,
    source: &Map<String, Value>,
    mappings: &BTreeMap<String, String>,
    canonical_ids: &BTreeSet<String>,
    final_ids: &BTreeSet<String>,
    high_water: u64,
) -> io::Result<()> {
    let mut restored = existing.clone();
    // Task identity does not establish a reference's origin. Targets already
    // present before migration are canonical, even if their legacy IDs collide.
    if existing.get("parent_id") == source.get("parent_id")
        && let Some(mapped) = source
            .get("parent_id")
            .and_then(Value::as_str)
            .filter(|id| !canonical_ids.contains(*id))
            .and_then(|id| mappings.get(id))
    {
        restored.insert("parent_id".into(), json!(mapped));
    }
    if let (Some(items), Some(original)) = (
        restored.get_mut("blocked_by").and_then(Value::as_array_mut),
        source.get("blocked_by").and_then(Value::as_array),
    ) {
        for item in items {
            if original.contains(item)
                && let Some(mapped) = item
                    .as_str()
                    .filter(|id| !canonical_ids.contains(*id))
                    .and_then(|id| mappings.get(id))
            {
                *item = json!(mapped);
            }
        }
    }
    // A failed earlier migration may have written a numeric reference without
    // its payload. If the reference was already renumbered, the raw source ID
    // cannot prove the missing mapping, whether it was canonical or not.
    for key in ["parent_id", "blocked_by"] {
        let remapped_source = reference_ids(source, key).any(|id| {
            !canonical_ids.contains(id) && mappings.get(id).is_some_and(|mapped| mapped != id)
        });
        let missing_reserved_reference = reference_ids(&restored, key).any(|id| {
            !final_ids.contains(id)
                && id
                    .strip_prefix('T')
                    .and_then(|number| number.parse::<u64>().ok())
                    .is_some_and(|number| number <= high_water)
        });
        if remapped_source && missing_reserved_reference {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "legacy task reference mapping is ambiguous; existing files were retained",
            ));
        }
    }
    if restored == *existing {
        return Ok(());
    }
    let same_client = source
        .get("client_request_id")
        .and_then(Value::as_str)
        .is_some_and(|id| {
            !id.is_empty() && existing.get("client_request_id") == source.get("client_request_id")
        });
    let same_creation = source
        .get("created_at")
        .and_then(Value::as_str)
        .is_some_and(|at| {
            !at.is_empty()
                && existing.get("created_at") == source.get("created_at")
                && existing.get("created_by") == source.get("created_by")
        });
    let mut existing_content = existing.clone();
    let mut source_content = source.clone();
    for key in ["id", "parent_id", "blocked_by"] {
        existing_content.remove(key);
        source_content.remove(key);
    }
    if !(same_client || same_creation || existing_content == source_content) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "legacy task identity is ambiguous; references were not remapped",
        ));
    }
    *existing = restored;
    Ok(())
}

fn reference_ids<'a>(task: &'a Map<String, Value>, key: &str) -> impl Iterator<Item = &'a str> {
    task.get(key)
        .into_iter()
        .flat_map(|value| {
            value
                .as_array()
                .map_or(std::slice::from_ref(value), Vec::as_slice)
        })
        .filter_map(Value::as_str)
}

fn rewrite_task_references(task: &mut Map<String, Value>, mappings: &BTreeMap<String, String>) {
    let mapped_parent = task
        .get("parent_id")
        .and_then(Value::as_str)
        .and_then(|parent| mappings.get(parent))
        .cloned();
    if let Some(mapped) = mapped_parent {
        task.insert("parent_id".into(), Value::String(mapped));
    }
    if let Some(items) = task.get_mut("blocked_by").and_then(Value::as_array_mut) {
        for item in items {
            if let Some(mapped) = item.as_str().and_then(|id| mappings.get(id)) {
                *item = Value::String(mapped.clone());
            }
        }
    }
}

fn task_id(task: &Map<String, Value>) -> Option<&str> {
    task.get("id").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ids_are_mapped_without_overwriting_python_tasks() {
        let mut canonical = ContextDoc::default();
        canonical.tasks.push(
            json!({"id":"T004","title":"existing"})
                .as_object()
                .cloned()
                .expect("task"),
        );
        let mut legacy = ContextDoc::default();
        legacy.tasks.push(
            json!({"id":"t_old","title":"new","parent_id":null})
                .as_object()
                .cloned()
                .expect("task"),
        );
        let (merged, mappings) = merge(canonical, legacy).expect("merge");
        assert_eq!(mappings["t_old"], "T005");
        assert_eq!(task_id(&merged.tasks[1]), Some("T005"));
    }

    #[test]
    fn migration_does_not_reuse_retired_canonical_task_numbers() {
        let canonical = ContextDoc {
            task_id_high_water: 4,
            ..ContextDoc::default()
        };
        let legacy = ContextDoc {
            tasks: vec![
                json!({"id":"T002", "title":"incoming task"})
                    .as_object()
                    .expect("task")
                    .clone(),
            ],
            ..ContextDoc::default()
        };
        let (merged, mappings) = merge(canonical, legacy).expect("merge");
        assert_eq!(mappings["T002"], "T005");
        assert_eq!(task_id(&merged.tasks[0]), Some("T005"));
        assert_eq!(merged.task_id_high_water, 5);
    }

    #[test]
    fn reference_recovery_uses_targets_present_before_migration() {
        let owner = json!({"id":"T004", "title":"owner", "client_request_id":"owner-request",
            "parent_id":"T002", "blocked_by":["T002", "T006"]})
        .as_object()
        .expect("owner")
        .clone();
        let canonical = ContextDoc {
            task_id_high_water: 4,
            tasks: vec![
                owner.clone(),
                json!({"id":"T002", "title":"canonical dependency"})
                    .as_object()
                    .expect("dependency")
                    .clone(),
            ],
            ..ContextDoc::default()
        };
        let legacy = ContextDoc {
            tasks: vec![
                json!({"id":"t_first", "title":"first import"})
                    .as_object()
                    .expect("first")
                    .clone(),
                json!({"id":"t_second", "title":"second import"})
                    .as_object()
                    .expect("second")
                    .clone(),
                owner,
                json!({"id":"T006", "title":"missing dependency"})
                    .as_object()
                    .expect("missing")
                    .clone(),
                json!({"id":"T002", "title":"unrelated legacy dependency"})
                    .as_object()
                    .expect("collision")
                    .clone(),
            ],
            ..ContextDoc::default()
        };
        let (actual, mappings) = merge(canonical, legacy).expect("recover missing reference");
        assert_eq!(mappings["t_second"], "T006");
        assert_eq!(mappings["T006"], "T007");
        assert_eq!(actual.tasks[0]["blocked_by"], json!(["T002", "T007"]));
        assert_eq!(actual.tasks[0]["parent_id"], "T002");
    }

    #[test]
    fn partial_migration_retry_restores_references_and_preserves_canonical_edits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = ContextPaths::new(temp.path());
        let legacy = ContextDoc {
            tasks: vec![
                json!({"id":"T001", "title":"parent", "client_request_id":"parent-request",
                    "blocked_by":["T002"], "status":"planned", "notes":"original"})
                .as_object()
                .expect("parent")
                .clone(),
                json!({"id":"T002", "title":"child", "client_request_id":"child-request",
                    "parent_id":"T001", "status":"planned"})
                .as_object()
                .expect("child")
                .clone(),
            ],
            ..ContextDoc::default()
        };
        write_json(&paths.legacy_file, &legacy).expect("legacy file");
        let before = yaml_storage::load(&paths).expect("read migration snapshot");
        let (merged, _) = merge(before.clone(), legacy).expect("prepare migration");
        // Become unavailable after the snapshot, so T001 commits before T002 fails.
        std::fs::create_dir_all(paths.tasks_dir.join("T002.yaml")).expect("block second task");
        assert!(yaml_storage::persist_diff(&paths, &before, &merged).is_err());
        assert!(!paths.migration_file.exists());
        std::fs::remove_dir(paths.tasks_dir.join("T002.yaml")).expect("unblock second task");
        let partial = yaml_storage::load(&paths).expect("partial state");
        assert_eq!(partial.task_id_high_water, 2);
        assert_eq!(partial.tasks.len(), 1);
        let mut edited = partial.clone();
        edited.tasks[0].insert("status".into(), json!("active"));
        edited.tasks[0].insert("notes".into(), json!("user edits"));
        edited.tasks[0].insert("parent_id".into(), json!("T901"));
        edited.tasks[0].insert("blocked_by".into(), json!(["T002", "T900"]));
        yaml_storage::persist_diff(&paths, &partial, &edited).expect("canonical edits");

        migrate_legacy_json(&paths).expect("retry migration");
        let actual = yaml_storage::load(&paths).expect("migrated state");
        assert_eq!(actual.task_id_high_water, 3);
        assert_eq!(actual.tasks.len(), 2);
        let parent = &actual.tasks[0];
        assert_eq!(parent["blocked_by"], json!(["T003", "T900"]));
        assert_eq!(parent["parent_id"], "T901");
        assert_eq!(parent["status"], "active");
        assert_eq!(parent["notes"], "user edits");
        assert_eq!(actual.tasks[1]["id"], "T003");
        assert_eq!(actual.tasks[1]["parent_id"], "T001");
        let once = actual.clone();
        migrate_legacy_json(&paths).expect("completed migration is idempotent");
        assert_eq!(
            yaml_storage::load(&paths).expect("final state").tasks,
            once.tasks
        );
    }

    #[test]
    fn matching_titles_do_not_merge_conflicting_request_identities() {
        let canonical_task = json!({"id":"T004", "title":"shared title",
            "client_request_id":"canonical-request", "blocked_by":["T002"], "notes":"canonical"})
        .as_object()
        .expect("canonical")
        .clone();
        let canonical = ContextDoc {
            task_id_high_water: 4,
            tasks: vec![canonical_task.clone()],
            ..ContextDoc::default()
        };
        let legacy =
            ContextDoc {
                tasks: vec![
            json!({"id":"T001", "title":"shared title", "client_request_id":"legacy-request",
                "blocked_by":["T002"]}).as_object().expect("legacy parent").clone(),
            json!({"id":"T002", "title":"new child", "parent_id":"T001"})
                .as_object().expect("legacy child").clone(),
        ],
                ..ContextDoc::default()
            };
        let (actual, mapping) = merge(canonical, legacy).expect("merge distinct tasks");
        assert_eq!(mapping["T001"], "T005");
        assert_eq!(mapping["T002"], "T006");
        assert_eq!(actual.tasks[0], canonical_task);
        assert_eq!(actual.tasks[1]["blocked_by"], json!(["T006"]));
        assert_eq!(actual.tasks[2]["parent_id"], "T005");
    }

    #[test]
    fn migration_reference_recovery_requires_more_than_a_matching_title() {
        let canonical = ContextDoc {
            task_id_high_water: 4,
            tasks: vec![
                json!({"id":"T004", "title":"shared title",
                "blocked_by":["T002"], "notes":"a different canonical task"})
                .as_object()
                .expect("canonical task")
                .clone(),
            ],
            ..ContextDoc::default()
        };
        let legacy = ContextDoc {
            tasks: vec![
                json!({"id":"T001", "title":"shared title", "blocked_by":["T002"]})
                    .as_object()
                    .expect("legacy parent")
                    .clone(),
                json!({"id":"T002", "title":"new child"})
                    .as_object()
                    .expect("legacy child")
                    .clone(),
            ],
            ..ContextDoc::default()
        };
        let error = merge(canonical, legacy).expect_err("ambiguous identity is rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("identity is ambiguous"));
    }

    #[test]
    fn migration_reference_recovery_without_client_ids_needs_creation_or_unchanged_content() {
        for creation_identity in [false, true] {
            let mut parent = json!({"id":"T001", "title":"parent", "blocked_by":["T002"],
                "parent_id":"T002", "notes":"original"})
            .as_object()
            .expect("parent")
            .clone();
            if creation_identity {
                parent.insert("created_at".into(), json!("2026-10-10T00:00:00Z"));
                parent.insert("created_by".into(), json!("user"));
            }
            let mut existing = parent.clone();
            existing.insert("parent_id".into(), json!("T900"));
            if creation_identity {
                existing.insert("notes".into(), json!("user edits"));
            }
            let canonical = ContextDoc {
                task_id_high_water: 2,
                tasks: vec![existing.clone()],
                ..ContextDoc::default()
            };
            let legacy = ContextDoc {
                tasks: vec![
                    parent,
                    json!({"id":"T002", "title":"child"})
                        .as_object()
                        .expect("child")
                        .clone(),
                ],
                ..ContextDoc::default()
            };
            let (actual, _) = merge(canonical, legacy).expect("recover verified task");
            assert_eq!(actual.tasks[0]["blocked_by"], json!(["T003"]));
            assert_eq!(actual.tasks[0]["parent_id"], "T900");
            assert_eq!(actual.tasks[0]["notes"], existing["notes"]);
        }
    }

    #[test]
    fn partial_migration_retry_rejects_a_lost_reference_mapping() {
        for (parent_id, child_id, high_water, first_parent, first_child, retry_child) in [
            ("t_parent", "t_child", 0, "T001", "T002", "T003"),
            ("T002", "T003", 4, "T005", "T006", "T007"),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let paths = ContextPaths::new(temp.path());
            let seed = ContextDoc {
                task_id_high_water: high_water,
                ..ContextDoc::default()
            };
            yaml_storage::persist_diff(&paths, &ContextDoc::default(), &seed)
                .expect("reserve existing task numbers");
            let legacy =
                ContextDoc {
                    tasks: vec![
                    json!({"id":parent_id, "title":"parent", "client_request_id":"parent-request",
                        "blocked_by":[child_id], "parent_id":child_id, "notes":"original"})
                    .as_object().expect("parent").clone(),
                    json!({"id":child_id, "title":"child", "client_request_id":"child-request"})
                        .as_object().expect("child").clone(),
                ],
                    ..ContextDoc::default()
                };
            write_json(&paths.legacy_file, &legacy).expect("legacy file");
            let before = yaml_storage::load(&paths).expect("migration snapshot");
            let (merged, _) = merge(before.clone(), legacy).expect("prepare migration");
            let missing_path = paths.tasks_dir.join(format!("{first_child}.yaml"));
            std::fs::create_dir_all(&missing_path).expect("block second task");
            assert!(yaml_storage::persist_diff(&paths, &before, &merged).is_err());
            std::fs::remove_dir(&missing_path).expect("unblock second task");
            let partial = yaml_storage::load(&paths).expect("partial migration");
            assert_eq!(partial.task_id_high_water, high_water + 2);
            assert_eq!(partial.tasks[0]["blocked_by"], json!([first_child]));
            assert_eq!(partial.tasks[0]["parent_id"], first_child);
            let mut edited = partial.clone();
            edited.tasks[0].insert("notes".into(), json!("user edits"));
            yaml_storage::persist_diff(&paths, &partial, &edited).expect("preserve user edits");
            let parent_path = paths.tasks_dir.join(format!("{first_parent}.yaml"));
            let task_bytes = std::fs::read(&parent_path).expect("task bytes");
            let version_bytes = std::fs::read(&paths.version_file).expect("version bytes");
            let legacy_bytes = std::fs::read(&paths.legacy_file).expect("legacy bytes");

            let error = migrate_legacy_json(&paths).expect_err("lost mapping is not guessed");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("reference mapping is ambiguous"));
            assert!(!paths.migration_file.exists());
            assert!(!paths.tasks_dir.join(format!("{retry_child}.yaml")).exists());
            assert_eq!(
                std::fs::read(&parent_path).expect("retained task"),
                task_bytes
            );
            assert_eq!(
                std::fs::read(&paths.version_file).expect("retained version"),
                version_bytes
            );
            assert_eq!(
                std::fs::read(&paths.legacy_file).expect("retained legacy"),
                legacy_bytes
            );
        }
    }
}

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
    let (mut merged, mappings) = merge(before.clone(), legacy);
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

fn merge(mut canonical: ContextDoc, legacy: ContextDoc) -> (ContextDoc, BTreeMap<String, String>) {
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
        .unwrap_or(0);
    let mut reserved = canonical
        .tasks
        .iter()
        .filter_map(task_id)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut mappings = BTreeMap::new();
    for task in &legacy.tasks {
        let old_id = task_id(task).unwrap_or_default();
        if let Some(existing) = equivalent_task_id(&canonical.tasks, task) {
            if !old_id.is_empty() {
                mappings.insert(old_id.to_owned(), existing);
            }
            continue;
        }
        let new_id = if yaml_storage::is_canonical_task_id(old_id) && !reserved.contains(old_id) {
            old_id.to_owned()
        } else {
            loop {
                next_number += 1;
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
        if canonical
            .tasks
            .iter()
            .filter_map(task_id)
            .any(|id| id == new_id)
        {
            continue;
        }
        task.insert("id".into(), Value::String(new_id));
        rewrite_task_references(&mut task, &mappings);
        canonical.tasks.push(task);
    }
    (canonical, mappings)
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
    let client_id = candidate.get("client_request_id").and_then(Value::as_str);
    let title = candidate.get("title").and_then(Value::as_str);
    tasks.iter().find_map(|task| {
        let same_client = client_id.is_some_and(|value| {
            !value.is_empty()
                && task.get("client_request_id").and_then(Value::as_str) == Some(value)
        });
        let same_title = title.is_some_and(|value| {
            !value.is_empty() && task.get("title").and_then(Value::as_str) == Some(value)
        });
        (same_client || same_title)
            .then(|| task_id(task).map(str::to_owned))
            .flatten()
    })
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
        let (merged, mappings) = merge(canonical, legacy);
        assert_eq!(mappings["t_old"], "T005");
        assert_eq!(task_id(&merged.tasks[1]), Some("T005"));
    }
}

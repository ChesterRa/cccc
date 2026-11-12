use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::io;

use super::model::ContextDoc;

const FINISHED_SINCE_REVIEW: &str = "finished_since_review_reminder";

fn is_done(task: &Map<String, Value>) -> bool {
    task.get("status").and_then(Value::as_str) == Some("done")
}

/// Top-level tasks that were not done before this write and are now. Restoring
/// an archived task that was done is not new work.
fn newly_finished_tasks(before: &ContextDoc, after: &ContextDoc) -> u64 {
    let was_done = before
        .tasks
        .iter()
        .filter(|task| super::task_is_completed(task))
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .collect::<HashSet<_>>();
    after
        .tasks
        .iter()
        .filter(|task| {
            is_done(task)
                && task
                    .get("parent_id")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                && task
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !was_done.contains(id))
        })
        .count() as u64
}

/// Adds the top-level tasks this write finished, whoever wrote it. A write that
/// takes reminders (`every`) and reaches the count restarts it and returns it.
/// Archiving or deleting finished tasks leaves the count alone.
pub(super) fn count_finished_tasks(
    before: &ContextDoc,
    document: &mut ContextDoc,
    every: Option<u64>,
) -> Option<u64> {
    let previous = document
        .meta
        .get(FINISHED_SINCE_REVIEW)
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let count = previous + newly_finished_tasks(before, document);
    let due = every.is_some_and(|every| count >= every);
    let next = if due { 0 } else { count };
    if next != previous {
        document
            .meta
            .insert(FINISHED_SINCE_REVIEW.into(), Value::from(next));
    }
    due.then_some(count)
}

pub fn apply_all(
    document: &mut ContextDoc,
    operations: &[Map<String, Value>],
    by: &str,
    mut authorize: impl FnMut(&ContextDoc, &Map<String, Value>) -> io::Result<()>,
) -> io::Result<Vec<Value>> {
    let mut changes = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        let name = operation.get("op").and_then(Value::as_str).unwrap_or("");
        authorize(document, operation)?;
        if name.starts_with("task.") {
            super::task_revision::check(&document.tasks, operation)?;
        }
        let tasks = name.starts_with("task.").then(|| document.tasks.clone());
        let mut change = json!({"index": index, "op": name, "detail": "applied"});
        if name == "task.create" {
            // The created task's id, or the existing task a replayed
            // `client_request_id` names, so the caller need not search for it.
            let created = super::task_apply::create(document, operation, by)?;
            change["task_id"] = Value::String(created.task_id);
            if created.replayed {
                change["replayed"] = Value::Bool(true);
            }
        } else {
            apply_one(document, name, operation, by)?;
        }
        if let Some(tasks) = tasks {
            super::task_revision::advance(&tasks, &mut document.tasks)?;
        }
        changes.push(change);
    }
    Ok(changes)
}

fn apply_one(
    document: &mut ContextDoc,
    name: &str,
    operation: &Map<String, Value>,
    by: &str,
) -> io::Result<()> {
    match name {
        "coordination.brief.update" => update_brief(document, operation, by),
        "task.update" => super::task_apply::update(document, operation),
        "task.move" => super::task_apply::move_task(document, operation),
        "task.restore" => super::task_apply::restore(document, operation),
        "task.delete" => super::task_apply::delete(document, operation),
        "agent_state.update" => super::agent_state_apply::update(document, operation),
        "agent_state.clear" => super::agent_state_apply::clear(document, operation),
        "meta.merge" => merge_meta(document, operation),
        _ => Err(io::Error::other(format!("unknown context op: {name}"))),
    }
}

fn update_brief(doc: &mut ContextDoc, op: &Map<String, Value>, by: &str) -> io::Result<()> {
    let brief = doc.coordination.entry("brief").or_insert_with(|| json!({}));
    let target = brief
        .as_object_mut()
        .ok_or_else(|| io::Error::other("invalid brief"))?;
    for key in [
        "objective",
        "current_focus",
        "constraints",
        "project_brief",
        "project_brief_stale",
    ] {
        if let Some(value) = op.get(key) {
            target.insert(key.into(), value.clone());
        }
    }
    target.insert("updated_by".into(), Value::String(by.into()));
    target.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

fn merge_meta(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let data = op
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| io::Error::other("data is required"))?;
    if let Some(value) = data.get("project_status") {
        doc.meta.insert("project_status".into(), value.clone());
    }
    Ok(())
}

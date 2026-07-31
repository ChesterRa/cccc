use cccc_contracts::utc_now;
use serde_json::{Map, Value};
use std::io;

use super::model::ContextDoc;

pub(super) fn create(doc: &mut ContextDoc, op: &Map<String, Value>, by: &str) -> io::Result<()> {
    let title = required(op, "title")?;
    let parent = op.get("parent_id").cloned().unwrap_or(Value::Null);
    if let Some(id) = parent.as_str() {
        find(doc, id)?;
    }
    let mut task = op
        .iter()
        .filter(|(key, _)| key.as_str() != "op")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    task.insert("id".into(), Value::String(next_id(&doc.tasks)));
    task.insert("title".into(), Value::String(title.into()));
    task.entry("status")
        .or_insert_with(|| Value::String("planned".into()));
    task.entry("task_type").or_insert_with(|| {
        Value::String(if parent.is_null() { "standard" } else { "free" }.into())
    });
    task.insert("created_by".into(), Value::String(by.into()));
    task.insert("created_at".into(), Value::String(utc_now()));
    task.insert("updated_at".into(), Value::String(utc_now()));
    doc.tasks.push(task);
    Ok(())
}

pub(super) fn update(
    doc: &mut ContextDoc,
    op: &Map<String, Value>,
    move_only: bool,
) -> io::Result<()> {
    let task = find_mut(doc, required(op, "task_id")?)?;
    let allowed: &[&str] = if move_only {
        &["status"]
    } else {
        &[
            "title",
            "outcome",
            "parent_id",
            "assignee",
            "priority",
            "blocked_by",
            "waiting_on",
            "handoff_to",
            "task_type",
            "notes",
            "checklist",
        ]
    };
    for key in allowed {
        if let Some(value) = op.get(*key) {
            task.insert((*key).into(), value.clone());
        }
    }
    task.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn restore(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let task = find_mut(doc, required(op, "task_id")?)?;
    let restored = task
        .remove("archived_from")
        .unwrap_or_else(|| Value::String("planned".into()));
    task.insert("status".into(), restored);
    task.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn delete(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let id = required(op, "task_id")?;
    find(doc, id)?;
    let mut removing = vec![id.to_owned()];
    let mut cursor = 0;
    while cursor < removing.len() {
        let parent = removing[cursor].clone();
        for task in &doc.tasks {
            let task_id = task.get("id").and_then(Value::as_str).unwrap_or("");
            if task.get("parent_id").and_then(Value::as_str) == Some(&parent)
                && !removing.iter().any(|candidate| candidate == task_id)
            {
                removing.push(task_id.to_owned());
            }
        }
        cursor += 1;
    }
    doc.tasks.retain(|task| {
        task.get("id")
            .and_then(Value::as_str)
            .is_none_or(|task_id| !removing.iter().any(|candidate| candidate == task_id))
    });
    Ok(())
}

fn next_id(tasks: &[Map<String, Value>]) -> String {
    let max = tasks
        .iter()
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .filter_map(|id| id.strip_prefix('T'))
        .filter_map(|number| number.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("T{:03}", max + 1)
}

fn find<'a>(doc: &'a ContextDoc, id: &str) -> io::Result<&'a Map<String, Value>> {
    doc.tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("task not found: {id}")))
}

fn find_mut<'a>(doc: &'a mut ContextDoc, id: &str) -> io::Result<&'a mut Map<String, Value>> {
    doc.tasks
        .iter_mut()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("task not found: {id}")))
}

fn required<'a>(op: &'a Map<String, Value>, key: &str) -> io::Result<&'a str> {
    op.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("{key} is required")))
}

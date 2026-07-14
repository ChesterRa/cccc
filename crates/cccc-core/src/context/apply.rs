use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use super::model::ContextDoc;

pub fn apply_all(
    document: &mut ContextDoc,
    operations: &[Map<String, Value>],
    by: &str,
) -> io::Result<Vec<Value>> {
    let mut changes = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        let name = operation.get("op").and_then(Value::as_str).unwrap_or("");
        apply_one(document, name, operation, by)?;
        changes.push(json!({"index": index, "op": name, "detail": "applied"}));
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
        "coordination.note.add" => add_note(document, operation, by),
        "task.create" => create_task(document, operation, by),
        "task.update" => update_task(document, operation, false),
        "task.move" => update_task(document, operation, true),
        "task.restore" => restore_task(document, operation),
        "task.delete" => delete_task(document, operation),
        "agent_state.update" => update_agent_state(document, operation),
        "agent_state.clear" => clear_agent_state(document, operation),
        "actor_notes.set" => set_actor_notes(document, operation),
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

fn add_note(doc: &mut ContextDoc, op: &Map<String, Value>, by: &str) -> io::Result<()> {
    let summary = required(op, "summary")?;
    let notes = doc.coordination.entry("notes").or_insert_with(|| json!([]));
    let target = notes
        .as_array_mut()
        .ok_or_else(|| io::Error::other("invalid notes"))?;
    target.push(json!({
        "id": Uuid::new_v4().simple().to_string(), "kind": string(op, "kind").unwrap_or("decision"),
        "summary": summary, "task_id": op.get("task_id").cloned().unwrap_or(Value::Null),
        "by": by, "created_at": utc_now(),
    }));
    if target.len() > 100 {
        target.drain(..target.len() - 100);
    }
    Ok(())
}

fn create_task(doc: &mut ContextDoc, op: &Map<String, Value>, by: &str) -> io::Result<()> {
    let title = required(op, "title")?;
    let parent = op.get("parent_id").cloned().unwrap_or(Value::Null);
    if let Some(id) = parent.as_str() {
        find_task(doc, id)?;
    }
    let mut task = copy_without_op(op);
    task.insert(
        "id".into(),
        Value::String(format!("t_{}", &Uuid::new_v4().simple().to_string()[..12])),
    );
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

fn update_task(doc: &mut ContextDoc, op: &Map<String, Value>, move_only: bool) -> io::Result<()> {
    let id = required(op, "task_id")?;
    let task = find_task_mut(doc, id)?;
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

fn restore_task(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let task = find_task_mut(doc, required(op, "task_id")?)?;
    let restored = task
        .remove("archived_from")
        .unwrap_or_else(|| Value::String("planned".into()));
    task.insert("status".into(), restored);
    Ok(())
}

fn delete_task(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let task = find_task_mut(doc, required(op, "task_id")?)?;
    let previous = task
        .get("status")
        .cloned()
        .unwrap_or_else(|| Value::String("planned".into()));
    task.insert("archived_from".into(), previous);
    task.insert("status".into(), Value::String("archived".into()));
    Ok(())
}

fn update_agent_state(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let actor = required(op, "actor_id")?;
    let state = doc.agent_states.entry(actor.into()).or_default();
    for (key, value) in op {
        if key != "op" && key != "actor_id" {
            state.insert(key.clone(), value.clone());
        }
    }
    state.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

fn clear_agent_state(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    doc.agent_states.remove(required(op, "actor_id")?);
    Ok(())
}

fn set_actor_notes(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let actor = required(op, "actor_id")?;
    doc.actor_notes.insert(
        actor.into(),
        op.get("notes").cloned().unwrap_or(Value::Null),
    );
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

fn find_task<'a>(doc: &'a ContextDoc, id: &str) -> io::Result<&'a Map<String, Value>> {
    doc.tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("task not found: {id}")))
}
fn find_task_mut<'a>(doc: &'a mut ContextDoc, id: &str) -> io::Result<&'a mut Map<String, Value>> {
    doc.tasks
        .iter_mut()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| io::Error::other(format!("task not found: {id}")))
}
fn required<'a>(op: &'a Map<String, Value>, key: &str) -> io::Result<&'a str> {
    string(op, key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("{key} is required")))
}
fn string<'a>(op: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    op.get(key).and_then(Value::as_str)
}
fn copy_without_op(op: &Map<String, Value>) -> Map<String, Value> {
    op.iter()
        .filter(|(key, _)| key.as_str() != "op")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

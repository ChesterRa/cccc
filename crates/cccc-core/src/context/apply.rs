use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};
use std::io;

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
        "task.create" => super::task_apply::create(document, operation, by),
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

fn add_note(doc: &mut ContextDoc, op: &Map<String, Value>, by: &str) -> io::Result<()> {
    let summary = required(op, "summary")?;
    let kind = string(op, "kind").unwrap_or("decision");
    let key = match kind {
        "decision" => "recent_decisions",
        "handoff" => "recent_handoffs",
        _ => return Err(io::Error::other("note kind must be decision or handoff")),
    };
    let notes = doc.coordination.entry(key).or_insert_with(|| json!([]));
    let target = notes
        .as_array_mut()
        .ok_or_else(|| io::Error::other("invalid notes"))?;
    target.push(json!({
        "at": utc_now(),
        "summary": summary, "task_id": op.get("task_id").cloned().unwrap_or(Value::Null),
        "by": by,
    }));
    if target.len() > 100 {
        target.drain(..target.len() - 100);
    }
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

fn required<'a>(op: &'a Map<String, Value>, key: &str) -> io::Result<&'a str> {
    string(op, key)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("{key} is required")))
}
fn string<'a>(op: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    op.get(key).and_then(Value::as_str)
}

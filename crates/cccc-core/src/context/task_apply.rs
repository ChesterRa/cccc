use cccc_contracts::utc_now;
use serde_json::{Map, Value};
use std::io;

use super::model::ContextDoc;

const TASK_STATUSES: &[&str] = &["planned", "active", "done", "archived"];
const TASK_TYPES: &[&str] = &["free", "standard", "optimization"];
const WAITING_ON: &[&str] = &["none", "user", "actor", "external"];

/// The task a `task.create` names: the one it created, or the one the same
/// caller already created under the same `client_request_id` (`replayed`).
pub(super) struct CreatedTask {
    pub(super) task_id: String,
    pub(super) replayed: bool,
}

pub(super) fn create(
    doc: &mut ContextDoc,
    op: &Map<String, Value>,
    by: &str,
) -> io::Result<CreatedTask> {
    let client_request_id = nullable_id(op, "client_request_id")?;
    if let Some(task_id) = client_request_id
        .as_deref()
        .and_then(|request_id| requested_task(doc, request_id, by))
    {
        return Ok(CreatedTask {
            task_id,
            replayed: true,
        });
    }
    let title = required(op, "title")?.trim().to_owned();
    let parent = match (
        nullable_id(op, "parent_id")?,
        nullable_id(op, "parent_ref")?,
    ) {
        (Some(_), Some(_)) => {
            return Err(io::Error::other(
                "task.create takes parent_id or parent_ref, not both",
            ));
        }
        (Some(id), None) => {
            find(doc, &id)?;
            Some(id)
        }
        (None, Some(reference)) => Some(referenced_task(doc, &reference, by)?),
        (None, None) => None,
    };
    let blocked_by = blocked_by_with_refs(doc, op, by)?;
    let status = enum_value(op, "status", TASK_STATUSES)?.unwrap_or("planned");
    let task_type = enum_value(op, "task_type", TASK_TYPES)?.unwrap_or(if parent.is_none() {
        "standard"
    } else {
        "free"
    });
    let waiting_on = enum_value(op, "waiting_on", WAITING_ON)?;
    let mut task = op.clone();
    for key in ["op", "if_task_rev", "rev", "parent_ref", "blocked_by_refs"] {
        task.remove(key);
    }
    let number = doc
        .task_id_high_water
        .max(maximum_task_number(&doc.tasks))
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "task number exhausted"))?;
    let task_id = format!("T{number:03}");
    task.insert("id".into(), Value::String(task_id.clone()));
    task.insert("title".into(), Value::String(title));
    task.insert("status".into(), Value::String(status.into()));
    task.insert("task_type".into(), Value::String(task_type.into()));
    match parent {
        Some(parent_id) => task.insert("parent_id".into(), Value::String(parent_id)),
        None => task.remove("parent_id"),
    };
    if let Some(value) = blocked_by {
        task.insert("blocked_by".into(), value);
    }
    match client_request_id {
        Some(request_id) => task.insert("client_request_id".into(), Value::String(request_id)),
        None => task.remove("client_request_id"),
    };
    if let Some(value) = waiting_on {
        task.insert("waiting_on".into(), Value::String(value.into()));
    }
    task.insert("created_by".into(), Value::String(by.into()));
    task.insert("created_at".into(), Value::String(utc_now()));
    task.insert("updated_at".into(), Value::String(utc_now()));
    doc.tasks.push(task);
    doc.task_id_high_water = number;
    Ok(CreatedTask {
        task_id,
        replayed: false,
    })
}

/// The task `by` created under `client_request_id`, if any.
fn requested_task(doc: &ContextDoc, request_id: &str, by: &str) -> Option<String> {
    if request_id.is_empty() {
        return None;
    }
    doc.tasks
        .iter()
        .find(|task| {
            task.get("client_request_id").and_then(Value::as_str) == Some(request_id)
                && task.get("created_by").and_then(Value::as_str) == Some(by)
        })
        .and_then(|task| task.get("id").and_then(Value::as_str))
        .map(str::to_owned)
}

/// A `parent_ref` or `blocked_by_refs` entry: the `client_request_id` of a task
/// the same caller created, earlier in this batch or before it.
fn referenced_task(doc: &ContextDoc, reference: &str, by: &str) -> io::Result<String> {
    requested_task(doc, reference, by).ok_or_else(|| {
        io::Error::other(format!(
            "task ref {reference} names no task created by {by}"
        ))
    })
}

/// `blocked_by` with each `blocked_by_refs` entry resolved and appended.
fn blocked_by_with_refs(
    doc: &ContextDoc,
    op: &Map<String, Value>,
    by: &str,
) -> io::Result<Option<Value>> {
    let Some(references) = op.get("blocked_by_refs") else {
        return Ok(None);
    };
    let references = references
        .as_array()
        .filter(|items| items.iter().all(Value::is_string))
        .ok_or_else(|| io::Error::other("blocked_by_refs must be an array of strings"))?;
    let mut blocked_by = match op.get("blocked_by") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => {
            return Err(io::Error::other(
                "blocked_by must be an array when blocked_by_refs is given",
            ));
        }
    };
    for reference in references.iter().filter_map(Value::as_str) {
        let task_id = Value::String(referenced_task(doc, reference.trim(), by)?);
        if !blocked_by.contains(&task_id) {
            blocked_by.push(task_id);
        }
    }
    Ok(Some(Value::Array(blocked_by)))
}

pub(super) fn update(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let task_id = required(op, "task_id")?.trim().to_owned();
    find(doc, &task_id)?;
    let title = op
        .get("title")
        .map(|_| required(op, "title").map(|value| value.trim().to_owned()))
        .transpose()?;
    let parent = op
        .contains_key("parent_id")
        .then(|| nullable_id(op, "parent_id"))
        .transpose()?;
    if let Some(Some(parent_id)) = parent.as_ref() {
        find(doc, parent_id)?;
        if creates_parent_cycle(doc, &task_id, parent_id) {
            return Err(io::Error::other("task parent would create a cycle"));
        }
    }
    let waiting_on = enum_value(op, "waiting_on", WAITING_ON)?;
    let task_type = enum_value(op, "task_type", TASK_TYPES)?;
    let task = find_mut(doc, &task_id)?;
    let allowed = [
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
    ];
    for key in allowed {
        if let Some(value) = op.get(key) {
            task.insert(key.into(), value.clone());
        }
    }
    if let Some(value) = title {
        task.insert("title".into(), Value::String(value));
    }
    if let Some(parent) = parent {
        match parent {
            Some(parent_id) => task.insert("parent_id".into(), Value::String(parent_id)),
            None => task.remove("parent_id"),
        };
    }
    if let Some(value) = waiting_on {
        task.insert("waiting_on".into(), Value::String(value.into()));
    }
    if let Some(value) = task_type {
        task.insert("task_type".into(), Value::String(value.into()));
    }
    task.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn move_task(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let unexpected = op
        .keys()
        .filter(|key| !matches!(key.as_str(), "op" | "task_id" | "status" | "if_task_rev"))
        .cloned()
        .collect::<Vec<_>>();
    if !unexpected.is_empty() {
        return Err(io::Error::other(format!(
            "task.move only accepts task_id and status: {}",
            unexpected.join(", ")
        )));
    }
    let task_id = required(op, "task_id")?;
    let status = enum_value(op, "status", TASK_STATUSES)?
        .ok_or_else(|| io::Error::other("status is required"))?;
    let task = find_mut(doc, task_id)?;
    let previous = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("planned")
        .to_owned();
    if previous == status {
        return Ok(());
    }
    if status == "archived" {
        task.insert("archived_from".into(), Value::String(previous));
    } else {
        task.remove("archived_from");
    }
    task.insert("status".into(), Value::String(status.into()));
    task.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn restore(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let task = find_mut(doc, required(op, "task_id")?)?;
    if task.get("status").and_then(Value::as_str) != Some("archived") {
        return Err(io::Error::other("task.restore requires an archived task"));
    }
    let restored = task
        .remove("archived_from")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|status| TASK_STATUSES.contains(&status.as_str()) && status != "archived")
        .unwrap_or_else(|| "planned".into());
    task.insert("status".into(), Value::String(restored));
    task.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn delete(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let id = required(op, "task_id")?;
    find(doc, id)?;
    let removing = doc.task_subtree_ids(id);
    if let Some(task) = doc.tasks.iter().find(|task| {
        task.get("id")
            .and_then(Value::as_str)
            .is_some_and(|task_id| removing.iter().any(|candidate| candidate == task_id))
            && !is_unexecuted(task)
    }) {
        let task_id = task.get("id").and_then(Value::as_str).unwrap_or(id);
        return Err(io::Error::other(format!(
            "task.delete preserves execution history at {task_id}"
        )));
    }
    doc.tasks.retain(|task| {
        task.get("id")
            .and_then(Value::as_str)
            .is_none_or(|task_id| !removing.iter().any(|candidate| candidate == task_id))
    });
    Ok(())
}

pub(super) fn maximum_task_number(tasks: &[Map<String, Value>]) -> u64 {
    tasks
        .iter()
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .filter_map(|id| id.strip_prefix('T'))
        .filter_map(|number| number.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn is_unexecuted(task: &Map<String, Value>) -> bool {
    match task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("planned")
    {
        "planned" => true,
        "archived" => task
            .get("archived_from")
            .and_then(Value::as_str)
            .is_none_or(|status| status.is_empty() || status == "planned"),
        _ => false,
    }
}

fn creates_parent_cycle(doc: &ContextDoc, task_id: &str, parent_id: &str) -> bool {
    let mut cursor = Some(parent_id);
    let mut seen = std::collections::BTreeSet::new();
    while let Some(current) = cursor {
        if current == task_id || !seen.insert(current.to_owned()) {
            return true;
        }
        cursor = doc
            .tasks
            .iter()
            .find(|task| task.get("id").and_then(Value::as_str) == Some(current))
            .and_then(|task| task.get("parent_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
    }
    false
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

fn nullable_id(op: &Map<String, Value>, key: &str) -> io::Result<Option<String>> {
    match op.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            Ok((!value.trim().is_empty()).then(|| value.trim().to_owned()))
        }
        Some(_) => Err(io::Error::other(format!("{key} must be a string or null"))),
    }
}

fn enum_value<'a>(
    op: &'a Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> io::Result<Option<&'a str>> {
    let Some(value) = op.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| allowed.contains(value))
        .ok_or_else(|| io::Error::other(format!("invalid {key}")))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn operation(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("operation")
    }

    #[test]
    fn structural_validation_rejects_invalid_task_mutations() {
        let mut doc = ContextDoc::default();
        create(
            &mut doc,
            &operation(json!({"op":"task.create","title":"root"})),
            "user",
        )
        .expect("root");
        create(
            &mut doc,
            &operation(json!({"op":"task.create","title":"child","parent_id":"T001"})),
            "user",
        )
        .expect("child");
        assert_eq!(doc.tasks[0]["task_type"], "standard");
        assert_eq!(doc.tasks[1]["task_type"], "free");

        for invalid in [
            json!({"op":"task.create","title":"bad","status":"bogus"}),
            json!({"op":"task.create","title":"bad","task_type":"bogus"}),
            json!({"op":"task.create","title":"bad","waiting_on":"bogus"}),
        ] {
            assert!(create(&mut doc, &operation(invalid), "user").is_err());
        }
        for invalid in [
            json!({"op":"task.update","task_id":"T001","title":"  "}),
            json!({"op":"task.update","task_id":"T001","parent_id":"T999"}),
            json!({"op":"task.update","task_id":"T001","parent_id":"T002"}),
            json!({"op":"task.update","task_id":"T001","task_type":"bogus"}),
        ] {
            assert!(update(&mut doc, &operation(invalid)).is_err());
        }
        assert!(
            move_task(
                &mut doc,
                &operation(json!({"op":"task.move","task_id":"T001","status":"bogus"}))
            )
            .is_err()
        );
        assert!(
            move_task(
                &mut doc,
                &operation(json!({
                    "op":"task.move","task_id":"T001","status":"active","notes":"ignored"
                }))
            )
            .is_err()
        );
        assert_eq!(doc.tasks[0]["title"], "root");
        assert_eq!(doc.tasks[0]["status"], "planned");
        assert!(doc.tasks[0].get("parent_id").is_none());
    }

    #[test]
    fn archive_restore_and_delete_preserve_task_history() {
        let mut doc = ContextDoc::default();
        create(
            &mut doc,
            &operation(json!({"op":"task.create","title":"task"})),
            "user",
        )
        .expect("create");
        assert!(
            restore(
                &mut doc,
                &operation(json!({"op":"task.restore","task_id":"T001"}))
            )
            .is_err()
        );
        move_task(
            &mut doc,
            &operation(json!({"op":"task.move","task_id":"T001","status":"active"})),
        )
        .expect("active");
        assert!(
            delete(
                &mut doc,
                &operation(json!({"op":"task.delete","task_id":"T001"}))
            )
            .is_err()
        );
        move_task(
            &mut doc,
            &operation(json!({"op":"task.move","task_id":"T001","status":"archived"})),
        )
        .expect("archive");
        assert_eq!(doc.tasks[0]["archived_from"], "active");
        restore(
            &mut doc,
            &operation(json!({"op":"task.restore","task_id":"T001"})),
        )
        .expect("restore");
        assert_eq!(doc.tasks[0]["status"], "active");
        assert!(doc.tasks[0].get("archived_from").is_none());

        let mut disposable = ContextDoc::default();
        create(
            &mut disposable,
            &operation(json!({"op":"task.create","title":"root"})),
            "user",
        )
        .expect("root");
        create(
            &mut disposable,
            &operation(json!({"op":"task.create","title":"child","parent_id":"T001"})),
            "user",
        )
        .expect("child");
        delete(
            &mut disposable,
            &operation(json!({"op":"task.delete","task_id":"T001"})),
        )
        .expect("delete planned subtree");
        assert!(disposable.tasks.is_empty());
    }
}

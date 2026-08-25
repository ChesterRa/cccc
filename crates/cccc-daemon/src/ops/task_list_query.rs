use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

use crate::dispatch::{OpError, string_arg};

pub(super) fn validated_filter(
    request: &DaemonRequest,
    name: &str,
    allowed: &[&str],
) -> Result<Option<String>, OpError> {
    let value = string_arg(request, name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if !allowed.contains(&value) {
        return Err(OpError::new(
            "invalid_args",
            format!("invalid {name}: {value}"),
        ));
    }
    Ok(Some(value.to_owned()))
}

pub(super) fn matches_task(
    task: &Map<String, Value>,
    status: Option<&str>,
    attention: Option<&str>,
    query: &str,
    assignee: &str,
) -> bool {
    if status.is_some_and(|wanted| task_status(task) != wanted) {
        return false;
    }
    let task_assignee = text(task, "assignee");
    if assignee == "__unassigned__" && !task_assignee.is_empty() {
        return false;
    }
    if !assignee.is_empty() && assignee != "__unassigned__" && task_assignee != assignee {
        return false;
    }
    if attention.is_some_and(|wanted| !matches_attention(task, wanted)) {
        return false;
    }
    query.is_empty()
        || [
            "id",
            "title",
            "outcome",
            "notes",
            "assignee",
            "priority",
            "handoff_to",
        ]
        .into_iter()
        .any(|key| text(task, key).to_lowercase().contains(query))
}

pub(super) fn facets(tasks: &[Map<String, Value>]) -> Value {
    let mut statuses = BTreeMap::<String, usize>::new();
    let mut assignees = BTreeSet::new();
    let mut blocked_count = 0;
    let mut waiting_user = 0;
    let mut handoffs = 0;
    let mut unassigned = 0;
    for task in tasks {
        *statuses.entry(task_status(task).into()).or_default() += 1;
        if task_status(task) == "archived" {
            continue;
        }
        let assignee = text(task, "assignee");
        if assignee.is_empty() {
            unassigned += 1
        } else {
            assignees.insert(assignee.to_owned());
        }
        if task_status(task) != "done" {
            blocked_count += usize::from(blocked(task));
            waiting_user += usize::from(text(task, "waiting_on") == "user");
            handoffs += usize::from(!text(task, "handoff_to").is_empty());
        }
    }
    json!({
        "status_counts":statuses,
        "blocked":blocked_count,
        "waiting_user":waiting_user,
        "pending_handoffs":handoffs,
        "unassigned":unassigned,
        "assignees":assignees,
    })
}

pub(super) fn sort_tasks(tasks: &mut [Map<String, Value>], status: Option<&str>) {
    let date_key = if status == Some("planned") {
        "created_at"
    } else {
        "updated_at"
    };
    tasks.sort_by(|left, right| {
        text(right, date_key)
            .cmp(text(left, date_key))
            .then_with(|| task_number(right).cmp(&task_number(left)))
    });
}

pub(super) fn delete_info(tasks: &[Map<String, Value>], root: &str) -> Value {
    let mut ids = vec![root.to_owned()];
    let mut cursor = 0;
    while cursor < ids.len() {
        let parent = ids[cursor].clone();
        for task in tasks
            .iter()
            .filter(|task| text(task, "parent_id") == parent)
        {
            let id = text(task, "id");
            if !id.is_empty() && !ids.iter().any(|candidate| candidate == id) {
                ids.push(id.to_owned());
            }
        }
        cursor += 1;
    }
    let blocked = tasks
        .iter()
        .find(|task| ids.iter().any(|id| id == text(task, "id")) && !unexecuted(task));
    let reason = blocked.map_or("", |task| {
        if text(task, "id") == root {
            "self_history"
        } else {
            "subtree_history"
        }
    });
    json!({"allowed":blocked.is_none(),"total":ids.len(),"reason":reason})
}

fn matches_attention(task: &Map<String, Value>, wanted: &str) -> bool {
    let status = task_status(task);
    match wanted {
        "unassigned" => status != "archived" && text(task, "assignee").is_empty(),
        "blocked" => !matches!(status, "done" | "archived") && blocked(task),
        "waiting_user" => {
            !matches!(status, "done" | "archived") && text(task, "waiting_on") == "user"
        }
        "handoff" => !matches!(status, "done" | "archived") && !text(task, "handoff_to").is_empty(),
        _ => true,
    }
}

fn blocked(task: &Map<String, Value>) -> bool {
    task.get("blocked_by")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || matches!(text(task, "waiting_on"), "actor" | "external")
}

fn unexecuted(task: &Map<String, Value>) -> bool {
    task_status(task) == "planned"
        || (task_status(task) == "archived"
            && matches!(text(task, "archived_from"), "" | "planned"))
}

pub(super) fn task_status(task: &Map<String, Value>) -> &str {
    let status = text(task, "status").trim();
    if status.is_empty() { "planned" } else { status }
}

pub(super) fn task_number(task: &Map<String, Value>) -> u64 {
    text(task, "id")
        .strip_prefix('T')
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

pub(super) fn text<'a>(task: &'a Map<String, Value>, key: &str) -> &'a str {
    task.get(key).and_then(Value::as_str).unwrap_or("")
}

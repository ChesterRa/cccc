use cccc_core::context::ContextDoc;
use serde_json::{Map, Value, json};

pub(super) fn project(document: ContextDoc, version: String, detail: &str) -> Value {
    if detail == "overview" {
        return context_overview::project(document, version);
    }
    let tasks = document.tasks;
    let tasks_version = format!("tasksv:{}", document.tasks_revision);
    let full = detail == "full";
    let attention = attention(&tasks, full);
    let coordination = coordination(&document.coordination, &tasks, full);
    let mut result = Map::from_iter([
        ("version".into(), Value::String(version)),
        ("tasks_version".into(), Value::String(tasks_version)),
        ("coordination".into(), Value::Object(coordination)),
        ("agent_states".into(), agent_states(document.agent_states)),
        ("actors_runtime".into(), Value::Array(Vec::new())),
        ("attention".into(), attention.clone()),
        ("tasks_summary".into(), task_summary(&tasks, &attention)),
        ("meta".into(), Value::Object(document.meta)),
    ]);
    if detail == "full" {
        result.insert("board".into(), board(&tasks));
    }
    Value::Object(result)
}

fn coordination(
    source: &Map<String, Value>,
    tasks: &[Map<String, Value>],
    full: bool,
) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert(
        "brief".into(),
        source.get("brief").cloned().unwrap_or_else(|| json!({})),
    );
    result.insert(
        "tasks".into(),
        Value::Array(
            tasks
                .iter()
                .map(|task| {
                    if full {
                        Value::Object(task.clone())
                    } else {
                        Value::Object(summary_task(task))
                    }
                })
                .collect(),
        ),
    );
    if full {
        for key in ["recent_decisions", "recent_handoffs"] {
            result.insert(
                key.into(),
                source.get(key).cloned().unwrap_or_else(|| json!([])),
            );
        }
    }
    result
}

fn summary_task(task: &Map<String, Value>) -> Map<String, Value> {
    const FIELDS: [&str; 18] = [
        "id",
        "title",
        "outcome",
        "parent_id",
        "status",
        "archived_from",
        "assignee",
        "priority",
        "blocked_by",
        "waiting_on",
        "handoff_to",
        "task_type",
        "notes",
        "checklist",
        "created_at",
        "updated_at",
        "progress",
        "is_root",
    ];
    FIELDS
        .into_iter()
        .filter_map(|field| {
            task.get(field)
                .filter(|value| !value.is_null())
                .cloned()
                .map(|value| (field.into(), value))
        })
        .collect()
}

fn agent_states(states: std::collections::BTreeMap<String, Map<String, Value>>) -> Value {
    Value::Array(
        states
            .into_iter()
            .map(|(actor_id, mut state)| {
                state.entry("id").or_insert_with(|| Value::String(actor_id));
                Value::Object(state)
            })
            .collect(),
    )
}

fn status(task: &Map<String, Value>) -> &str {
    task.get("status")
        .and_then(Value::as_str)
        .unwrap_or("planned")
}

fn is_blocked(task: &Map<String, Value>) -> bool {
    task.get("blocked_by")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || matches!(
            task.get("waiting_on").and_then(Value::as_str),
            Some("actor" | "external")
        )
}

fn task_summary(tasks: &[Map<String, Value>], attention: &Value) -> Value {
    let count = |wanted| tasks.iter().filter(|task| status(task) == wanted).count();
    let attention = attention.as_object();
    let attention_count = |key| {
        attention
            .and_then(|item| item.get(key))
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_array().map(|items| items.len() as u64))
            })
            .unwrap_or(0)
    };
    json!({
        "total":tasks.iter().filter(|task| status(task) != "archived").count(),
        "planned":count("planned"),
        "active":count("active"),
        "done":count("done"),
        "archived":count("archived"),
        "blocked":attention_count("blocked"),
        "waiting_user":attention_count("waiting_user"),
        "pending_handoffs":attention_count("pending_handoffs"),
    })
}

fn attention(tasks: &[Map<String, Value>], full: bool) -> Value {
    let live = tasks
        .iter()
        .filter(|task| !matches!(status(task), "done" | "archived"));
    let blocked = live.clone().filter(|task| is_blocked(task)).count();
    let waiting_user = live
        .clone()
        .filter(|task| task.get("waiting_on").and_then(Value::as_str) == Some("user"))
        .count();
    let pending_handoffs = live
        .filter(|task| {
            task.get("handoff_to")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        })
        .count();
    if full {
        let entries = |wanted: &str| {
            tasks
                .iter()
                .filter(|task| status(task) != "done" && status(task) != "archived")
                .filter(|task| match wanted {
                    "blocked" => is_blocked(task),
                    "waiting_user" => {
                        task.get("waiting_on").and_then(Value::as_str) == Some("user")
                    }
                    "pending_handoffs" => task
                        .get("handoff_to")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.is_empty()),
                    _ => false,
                })
                .cloned()
                .map(Value::Object)
                .collect::<Vec<_>>()
        };
        json!({
            "blocked":entries("blocked"),
            "waiting_user":entries("waiting_user"),
            "pending_handoffs":entries("pending_handoffs"),
        })
    } else {
        json!({
            "blocked":blocked,
            "waiting_user":waiting_user,
            "pending_handoffs":pending_handoffs,
        })
    }
}

fn board(tasks: &[Map<String, Value>]) -> Value {
    let mut result = Map::new();
    for wanted in ["planned", "active", "done", "archived"] {
        result.insert(
            wanted.into(),
            Value::Array(
                tasks
                    .iter()
                    .filter(|task| status(task) == wanted)
                    .cloned()
                    .map(Value::Object)
                    .collect(),
            ),
        );
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests;

#[path = "context_overview.rs"]
mod context_overview;

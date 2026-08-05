use cccc_core::context::ContextDoc;
use serde_json::{Map, Value, json};

pub(super) fn project(document: ContextDoc, version: String, detail: &str) -> Value {
    let tasks = document.tasks;
    let attention = attention(&tasks);
    let coordination = coordination(&document.coordination, &tasks, detail == "full");
    let mut result = Map::from_iter([
        ("version".into(), Value::String(version)),
        ("coordination".into(), Value::Object(coordination)),
        ("agent_states".into(), agent_states(document.agent_states)),
        ("actors_runtime".into(), Value::Array(Vec::new())),
        ("attention".into(), attention.clone()),
        ("tasks_summary".into(), task_summary(&tasks, &attention)),
        ("meta".into(), Value::Object(document.meta)),
    ]);
    if detail == "full" {
        result.insert("board".into(), board(&tasks));
        result.insert(
            "actor_notes".into(),
            serde_json::to_value(document.actor_notes).unwrap_or_else(|_| json!({})),
        );
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
        Value::Array(tasks.iter().cloned().map(Value::Object).collect()),
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

fn task_summary(tasks: &[Map<String, Value>], attention: &Value) -> Value {
    let count = |wanted| tasks.iter().filter(|task| status(task) == wanted).count();
    let attention = attention.as_object();
    json!({
        "total":tasks.iter().filter(|task| status(task) != "archived").count(),
        "planned":count("planned"),
        "active":count("active"),
        "done":count("done"),
        "archived":count("archived"),
        "blocked":attention.and_then(|item| item["blocked"].as_array()).map_or(0, Vec::len),
        "waiting_user":attention.and_then(|item| item["waiting_user"].as_array()).map_or(0, Vec::len),
        "pending_handoffs":attention.and_then(|item| item["pending_handoffs"].as_array()).map_or(0, Vec::len),
    })
}

fn attention(tasks: &[Map<String, Value>]) -> Value {
    let live = tasks
        .iter()
        .filter(|task| !matches!(status(task), "done" | "archived"));
    let blocked = live
        .clone()
        .filter(|task| {
            task.get("blocked_by")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        })
        .cloned()
        .map(Value::Object)
        .collect::<Vec<_>>();
    let waiting_user = live
        .clone()
        .filter(|task| task.get("waiting_on").and_then(Value::as_str) == Some("user"))
        .cloned()
        .map(Value::Object)
        .collect::<Vec<_>>();
    let pending_handoffs = live
        .filter(|task| {
            task.get("handoff_to")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        })
        .cloned()
        .map(Value::Object)
        .collect::<Vec<_>>();
    json!({
        "blocked":blocked,
        "waiting_user":waiting_user,
        "pending_handoffs":pending_handoffs,
    })
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

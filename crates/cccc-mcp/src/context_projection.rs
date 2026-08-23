use serde_json::{Map, Value, json};

/// Keep convenience context tools focused on their documented slice instead
/// of returning the daemon's complete context snapshot.
pub(crate) fn apply(
    tool_name: &str,
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("get");
    if !matches!(action, "get" | "list") {
        return;
    }
    match tool_name {
        "cccc_coordination" => project_coordination(result, arguments),
        "cccc_agent_state" => project_agent_state(result, arguments),
        _ => {}
    }
}

fn project_coordination(result: &mut Map<String, Value>, arguments: &Map<String, Value>) {
    let mut snapshot = std::mem::take(result);
    if !bool_argument(arguments, "include_archived", false) {
        hide_archived_tasks(&mut snapshot);
    }
    *result = Map::from_iter([
        ("version".into(), take_value(&mut snapshot, "version")),
        (
            "coordination".into(),
            take_object(&mut snapshot, "coordination"),
        ),
        ("attention".into(), take_object(&mut snapshot, "attention")),
        ("board".into(), take_object(&mut snapshot, "board")),
        (
            "tasks_summary".into(),
            take_object(&mut snapshot, "tasks_summary"),
        ),
    ]);
}

fn hide_archived_tasks(snapshot: &mut Map<String, Value>) {
    if let Some(coordination) = snapshot
        .get_mut("coordination")
        .and_then(Value::as_object_mut)
        && let Some(tasks) = coordination.get_mut("tasks").and_then(Value::as_array_mut)
    {
        tasks.retain(|task| {
            task.as_object().is_some_and(|task| {
                !task
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .eq_ignore_ascii_case("archived")
            })
        });
    }
    if let Some(board) = snapshot.get_mut("board").and_then(Value::as_object_mut)
        && !board.is_empty()
    {
        board.insert("archived".into(), Value::Array(Vec::new()));
    }
    if let Some(summary) = snapshot
        .get_mut("tasks_summary")
        .and_then(Value::as_object_mut)
        && !summary.is_empty()
    {
        let total = ["planned", "active", "done"]
            .into_iter()
            .map(|key| summary.get(key).and_then(Value::as_u64).unwrap_or(0))
            .fold(0_u64, u64::saturating_add);
        summary.insert("total".into(), json!(total));
    }
}

fn project_agent_state(result: &mut Map<String, Value>, arguments: &Map<String, Value>) {
    let mut snapshot = std::mem::take(result);
    let version = take_value(&mut snapshot, "version");
    let states = snapshot
        .remove("agent_states")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let target = arguments
        .get("actor_id")
        .or_else(|| arguments.get("agent_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let include_warm = bool_argument(arguments, "include_warm", true);

    let projection = if let Some(target) = target {
        let state = states.into_iter().find(|state| {
            state
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.trim().eq_ignore_ascii_case(target))
        });
        Map::from_iter([
            ("version".into(), version),
            (
                "agent_state".into(),
                state
                    .map(|state| compact_agent_state(state, include_warm))
                    .unwrap_or(Value::Null),
            ),
        ])
    } else {
        Map::from_iter([
            ("version".into(), version),
            (
                "agent_states".into(),
                Value::Array(
                    states
                        .into_iter()
                        .filter(Value::is_object)
                        .map(|state| compact_agent_state(state, include_warm))
                        .collect(),
                ),
            ),
        ])
    };
    *result = projection;
}

fn compact_agent_state(state: Value, include_warm: bool) -> Value {
    if include_warm {
        return state;
    }
    let state = state.as_object();
    Value::Object(Map::from_iter([
        (
            "id".into(),
            state
                .and_then(|state| state.get("id"))
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "hot".into(),
            state
                .and_then(|state| state.get("hot"))
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({})),
        ),
        (
            "updated_at".into(),
            state
                .and_then(|state| state.get("updated_at"))
                .cloned()
                .unwrap_or(Value::Null),
        ),
    ]))
}

fn bool_argument(arguments: &Map<String, Value>, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn take_value(snapshot: &mut Map<String, Value>, key: &str) -> Value {
    snapshot.remove(key).unwrap_or(Value::Null)
}

fn take_object(snapshot: &mut Map<String, Value>, key: &str) -> Value {
    snapshot
        .remove(key)
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    #[test]
    fn coordination_projection_drops_unrelated_context_and_hides_archived_tasks() {
        let mut result = object(json!({
            "version": "v1",
            "coordination": {"tasks": [
                {"id": "active", "status": "active"},
                {"id": "old", "status": "archived"}
            ]},
            "attention": {"needs_attention": []},
            "board": {"planned": [], "archived": [{"id": "old"}]},
            "tasks_summary": {"planned": 1, "active": 2, "done": 3, "archived": 4, "total": 10},
            "agent_states": [{"id": "peer", "warm": "very large"}],
            "actors_runtime": {"peer": {"history": "very large"}},
            "meta": {"unrelated": "very large"}
        }));

        apply("cccc_coordination", &mut result, &Map::new());

        assert_eq!(result.len(), 5);
        assert_eq!(
            result["coordination"]["tasks"]
                .as_array()
                .expect("tasks")
                .len(),
            1
        );
        assert_eq!(result["board"]["archived"], json!([]));
        assert_eq!(result["tasks_summary"]["total"], json!(6));
        assert!(!result.contains_key("agent_states"));
        assert!(!result.contains_key("actors_runtime"));
        assert!(!result.contains_key("meta"));
    }

    #[test]
    fn coordination_projection_can_include_archived_tasks() {
        let mut result = object(json!({
            "version": "v1",
            "coordination": {"tasks": [{"id": "old", "status": "archived"}]},
            "board": {"archived": [{"id": "old"}]},
            "tasks_summary": {"archived": 1, "total": 1}
        }));
        let arguments = object(json!({"include_archived": true}));

        apply("cccc_coordination", &mut result, &arguments);

        assert_eq!(
            result["coordination"]["tasks"]
                .as_array()
                .expect("tasks")
                .len(),
            1
        );
        assert_eq!(
            result["board"]["archived"]
                .as_array()
                .expect("archived")
                .len(),
            1
        );
        assert_eq!(result["tasks_summary"]["total"], json!(1));
    }

    #[test]
    fn agent_state_projection_selects_only_the_requested_actor() {
        let mut result = object(json!({
            "version": "v2",
            "agent_states": [
                {"id": "peer-a", "hot": {"focus": "a"}, "warm": {"large": "payload"}, "updated_at": "now"},
                {"id": "peer-b", "hot": {"focus": "b"}, "warm": {"large": "payload"}, "updated_at": "later"}
            ],
            "coordination": {"large": "payload"}
        }));
        let arguments = object(json!({"actor_id": "PEER-B", "include_warm": false}));

        apply("cccc_agent_state", &mut result, &arguments);

        assert_eq!(result.len(), 2);
        assert_eq!(result["agent_state"]["id"], json!("peer-b"));
        assert_eq!(result["agent_state"]["hot"]["focus"], json!("b"));
        assert_eq!(result["agent_state"]["updated_at"], json!("later"));
        assert!(result["agent_state"].get("warm").is_none());
        assert!(!result.contains_key("coordination"));
    }

    #[test]
    fn non_query_actions_are_not_projected() {
        let mut result = object(json!({"version": "v3", "coordination": {"ok": true}}));
        let expected = result.clone();
        let arguments = object(json!({"action": "update"}));

        apply("cccc_agent_state", &mut result, &arguments);

        assert_eq!(result, expected);
    }
}

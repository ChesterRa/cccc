use cccc_core::context::ContextDoc;
use serde_json::{Map, Value, json};

pub(super) fn project(document: ContextDoc, version: String) -> Value {
    let tasks_version = format!("tasksv:{}", document.tasks_revision);
    let mut coordination = Map::new();
    coordination.insert(
        "brief".into(),
        document
            .coordination
            .get("brief")
            .cloned()
            .unwrap_or_else(|| json!({})),
    );
    json!({
        "version":version,
        "tasks_version":tasks_version,
        "coordination":coordination,
        "agent_states":super::agent_states(document.agent_states),
        "actors_runtime":[],
        "meta":document.meta,
    })
}

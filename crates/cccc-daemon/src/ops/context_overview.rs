use cccc_core::context::ContextDoc;
use serde_json::{Map, Value, json};

pub(super) fn project(document: ContextDoc, version: String) -> Value {
    let tasks_version = format!("tasksv:{}", document.tasks_revision);
    let mut coordination = Map::new();
    for key in ["brief", "recent_decisions", "recent_handoffs"] {
        coordination.insert(
            key.into(),
            document
                .coordination
                .get(key)
                .cloned()
                .unwrap_or_else(|| if key == "brief" { json!({}) } else { json!([]) }),
        );
    }
    json!({
        "version":version,
        "tasks_version":tasks_version,
        "coordination":coordination,
        "agent_states":super::agent_states(document.agent_states),
        "actors_runtime":[],
        "meta":document.meta,
    })
}

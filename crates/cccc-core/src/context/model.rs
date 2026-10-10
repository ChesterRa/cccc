use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextDoc {
    pub v: u8,
    pub revision: u64,
    #[serde(default)]
    pub tasks_revision: u64,
    /// Reserved task numbers survive deletion; this is not a public projection field.
    #[serde(default, skip_serializing)]
    pub task_id_high_water: u64,
    pub updated_at: String,
    #[serde(default)]
    pub coordination: Map<String, Value>,
    #[serde(default)]
    pub tasks: Vec<Map<String, Value>>,
    #[serde(default)]
    pub agent_states: BTreeMap<String, Map<String, Value>>,
    #[serde(default)]
    pub meta: Map<String, Value>,
}

impl ContextDoc {
    /// The complete deletion target, shared by application and authorization.
    pub fn task_subtree_ids(&self, root_id: &str) -> Vec<String> {
        let mut subtree = vec![root_id.to_owned()];
        let mut cursor = 0;
        while cursor < subtree.len() {
            let parent = subtree[cursor].clone();
            for task in &self.tasks {
                let task_id = task.get("id").and_then(Value::as_str).unwrap_or("");
                if task.get("parent_id").and_then(Value::as_str) == Some(&parent)
                    && !subtree.iter().any(|candidate| candidate == task_id)
                {
                    subtree.push(task_id.to_owned());
                }
            }
            cursor += 1;
        }
        subtree
    }
}

impl Default for ContextDoc {
    fn default() -> Self {
        Self {
            v: 3,
            revision: 0,
            tasks_revision: 0,
            task_id_high_water: 0,
            updated_at: utc_now(),
            coordination: Map::new(),
            tasks: Vec::new(),
            agent_states: BTreeMap::new(),
            meta: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextSyncResult {
    pub context: ContextDoc,
    pub version: String,
    pub changes: Vec<Value>,
    pub dry_run: bool,
}

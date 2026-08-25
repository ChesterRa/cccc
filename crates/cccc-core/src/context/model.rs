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

impl Default for ContextDoc {
    fn default() -> Self {
        Self {
            v: 3,
            revision: 0,
            tasks_revision: 0,
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

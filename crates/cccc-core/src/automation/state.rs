use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;

use crate::GroupStore;
use crate::fs::{read_json, write_json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(super) struct RuntimeState {
    #[serde(default)]
    pub last_rule: BTreeMap<String, i64>,
    #[serde(default)]
    pub last_nudge: BTreeMap<String, i64>,
}

pub(super) fn load(store: &GroupStore, group_id: &str) -> io::Result<RuntimeState> {
    let path = store.state_dir(group_id)?.join("automation-runtime.json");
    if path.exists() {
        read_json(&path)
    } else {
        Ok(RuntimeState::default())
    }
}

pub(super) fn save(store: &GroupStore, group_id: &str, state: &RuntimeState) -> io::Result<()> {
    write_json(
        &store.state_dir(group_id)?.join("automation-runtime.json"),
        state,
    )
}

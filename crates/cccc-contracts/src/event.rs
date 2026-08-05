use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::utc_now;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    #[serde(default = "version")]
    pub v: u8,
    #[serde(default = "event_id")]
    pub id: String,
    #[serde(default = "utc_now")]
    pub ts: String,
    pub kind: String,
    pub group_id: String,
    #[serde(default)]
    pub scope_key: String,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub data: Map<String, Value>,
}

impl Event {
    #[must_use]
    pub fn new(kind: impl Into<String>, group_id: impl Into<String>) -> Self {
        Self {
            v: 1,
            id: event_id(),
            ts: utc_now(),
            kind: kind.into(),
            group_id: group_id.into(),
            scope_key: String::new(),
            by: String::new(),
            data: Map::new(),
        }
    }
}

const fn version() -> u8 {
    1
}
fn event_id() -> String {
    Uuid::new_v4().simple().to_string()
}

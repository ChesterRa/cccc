use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;

use crate::GroupStore;
use crate::fs::{read_json, write_json};
use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(super) struct RuntimeState {
    #[serde(default)]
    pub last_rule: BTreeMap<String, i64>,
    #[serde(default)]
    pub last_nudge: BTreeMap<String, i64>,
}

pub(super) fn load(store: &GroupStore, group_id: &str) -> io::Result<RuntimeState> {
    let state_dir = store.state_dir(group_id)?;
    let canonical = state_dir.join("automation.json");
    let legacy = state_dir.join("automation-runtime.json");
    let marker = state_dir.join(".rust-automation-migrated-v1");
    let mut state = RuntimeState::default();
    if canonical.exists() {
        let doc: Value = read_json(&canonical)?;
        if let Some(rules) = doc.get("rules").and_then(Value::as_object) {
            for (rule_id, entry) in rules {
                if let Some(timestamp) = entry
                    .get("last_fired_at")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
                {
                    state.last_rule.insert(rule_id.clone(), timestamp);
                }
            }
        }
        if let Some(actors) = doc.get("actors").and_then(Value::as_object) {
            for (actor_id, actor) in actors {
                let Some(items) = actor.get("nudge_items").and_then(Value::as_object) else {
                    continue;
                };
                for (event_id, entry) in items {
                    if let Some(timestamp) = entry
                        .get("last_nudged_at")
                        .and_then(Value::as_str)
                        .and_then(parse_timestamp)
                    {
                        state
                            .last_nudge
                            .insert(format!("{actor_id}:{event_id}"), timestamp);
                    }
                }
            }
        }
    }
    let migrate_legacy = legacy.exists() && !marker.exists();
    if migrate_legacy {
        let legacy: RuntimeState = read_json(&legacy)?;
        for (key, value) in legacy.last_rule {
            state
                .last_rule
                .entry(key)
                .and_modify(|current| *current = (*current).max(value))
                .or_insert(value);
        }
        for (key, value) in legacy.last_nudge {
            state
                .last_nudge
                .entry(key)
                .and_modify(|current| *current = (*current).max(value))
                .or_insert(value);
        }
        save(store, group_id, &state)?;
    }
    Ok(state)
}

pub(super) fn save(store: &GroupStore, group_id: &str, state: &RuntimeState) -> io::Result<()> {
    let state_dir = store.state_dir(group_id)?;
    let path = state_dir.join("automation.json");
    let mut doc = if path.exists() {
        read_json::<Value>(&path)?
    } else {
        json!({})
    };
    let root = object(&mut doc);
    root.insert("v".into(), json!(5));
    root.insert("updated_at".into(), json!(utc_now()));
    let rules = object(root.entry("rules").or_insert_with(|| json!({})));
    for (rule_id, timestamp) in &state.last_rule {
        let entry = object(rules.entry(rule_id.clone()).or_insert_with(|| json!({})));
        entry.insert("last_fired_at".into(), json!(format_timestamp(*timestamp)));
    }
    let actors = object(root.entry("actors").or_insert_with(|| json!({})));
    for (key, timestamp) in &state.last_nudge {
        let Some((actor_id, event_id)) = key.split_once(':') else {
            continue;
        };
        let actor = object(actors.entry(actor_id).or_insert_with(|| json!({})));
        let items = object(actor.entry("nudge_items").or_insert_with(|| json!({})));
        let item = object(items.entry(event_id).or_insert_with(|| json!({})));
        item.insert("last_nudged_at".into(), json!(format_timestamp(*timestamp)));
        item.entry("count").or_insert_with(|| json!(1));
    }
    write_json(&path, &doc)?;
    std::fs::write(
        state_dir.join(".rust-automation-migrated-v1"),
        b"migrated from automation-runtime.json\n",
    )
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value
        .as_object_mut()
        .expect("automation object initialized")
}

fn parse_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp())
}

fn format_timestamp(value: i64) -> String {
    chrono::DateTime::from_timestamp(value, 0)
        .map(|value| value.to_rfc3339())
        .unwrap_or_default()
}

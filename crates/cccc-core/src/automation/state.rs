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
    let canonical_exists = canonical.exists();
    if canonical_exists {
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
    let legacy_pending = legacy.exists() && !marker.exists();
    let migrate_legacy = legacy_pending && !canonical_exists;
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
    } else if legacy_pending {
        std::fs::write(
            marker,
            b"canonical automation.json supersedes automation-runtime.json\n",
        )?;
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

pub(super) fn reconcile_rules(
    store: &GroupStore,
    group_id: &str,
    previous: &[Value],
    current: &[Value],
) -> io::Result<()> {
    let path = store.state_dir(group_id)?.join("automation.json");
    if !path.exists() {
        return Ok(());
    }
    let mut doc = read_json::<Value>(&path)?;
    let Some(rules_state) = doc.get_mut("rules").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let previous = rules_by_id(previous);
    let current = rules_by_id(current);
    let mut changed = false;

    rules_state.retain(|rule_id, _| {
        let keep = current.contains_key(rule_id);
        changed |= !keep;
        keep
    });
    for (rule_id, rule) in current {
        let Some(entry) = rules_state.get_mut(&rule_id).and_then(Value::as_object_mut) else {
            continue;
        };
        let current_kind = trigger_text(rule, "kind");
        if current_kind != "at" {
            changed |= entry.remove("at_fired").is_some();
            if entry
                .get("last_slot_key")
                .and_then(Value::as_str)
                .is_some_and(|slot| slot.starts_with("at:"))
            {
                entry.remove("last_slot_key");
                changed = true;
            }
            continue;
        }
        let same_generation = previous.get(&rule_id).is_some_and(|previous| {
            trigger_text(previous, "kind") == "at"
                && trigger_text(previous, "at") == trigger_text(rule, "at")
        });
        if same_generation {
            continue;
        }
        changed |= entry.remove("at_fired").is_some();
        changed |= entry.remove("last_fired_at").is_some();
        if entry
            .get("last_slot_key")
            .and_then(Value::as_str)
            .is_some_and(|slot| slot.starts_with("at:"))
        {
            entry.remove("last_slot_key");
            changed = true;
        }
    }
    if changed {
        object(&mut doc).insert("updated_at".into(), json!(utc_now()));
        write_json(&path, &doc)?;
    }
    Ok(())
}

fn rules_by_id(rules: &[Value]) -> BTreeMap<String, &Value> {
    rules
        .iter()
        .filter_map(|rule| {
            rule.get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| (id.to_owned(), rule))
        })
        .collect()
}

fn trigger_text<'a>(rule: &'a Value, key: &str) -> &'a str {
    rule.get("trigger")
        .and_then(|trigger| trigger.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GroupStore, HomeLayout};

    #[test]
    fn canonical_state_does_not_reimport_unmarked_legacy_rules() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("automation", "").expect("group");
        let state_dir = store.state_dir(&group.group_id).expect("state dir");
        write_json(
            &state_dir.join("automation.json"),
            &json!({"v":5,"rules":{}}),
        )
        .expect("canonical state");
        write_json(
            &state_dir.join("automation-runtime.json"),
            &json!({"last_rule":{"retired-rule":1_700_000_000}}),
        )
        .expect("legacy state");

        let loaded = load(&store, &group.group_id).expect("load canonical state");

        assert!(
            !loaded.last_rule.contains_key("retired-rule"),
            "an existing canonical state is terminal and must not import a stale legacy rule"
        );
        assert!(state_dir.join(".rust-automation-migrated-v1").exists());
    }

    #[test]
    fn legacy_state_migrates_when_no_canonical_state_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("automation", "").expect("group");
        let state_dir = store.state_dir(&group.group_id).expect("state dir");
        write_json(
            &state_dir.join("automation-runtime.json"),
            &json!({"last_rule":{"legacy-rule":1_700_000_000}}),
        )
        .expect("legacy state");

        let loaded = load(&store, &group.group_id).expect("migrate legacy state");

        assert_eq!(loaded.last_rule.get("legacy-rule"), Some(&1_700_000_000));
        assert!(state_dir.join("automation.json").exists());
        assert!(state_dir.join(".rust-automation-migrated-v1").exists());
    }
}

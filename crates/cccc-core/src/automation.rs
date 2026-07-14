use cccc_contracts::{Event, GroupState, utc_now};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;

use crate::actors;
use crate::fs::{read_json, write_json};
use crate::inbox;
use crate::ledger;
use crate::{GroupDoc, GroupStore, HomeLayout};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RuntimeState {
    #[serde(default)]
    last_rule: BTreeMap<String, i64>,
    #[serde(default)]
    last_nudge: BTreeMap<String, i64>,
}

pub fn tick(home: &HomeLayout) -> io::Result<usize> {
    let store = GroupStore::new(home.clone())?;
    let mut emitted = 0;
    for meta in store.list()? {
        let group = match store.load(&meta.group_id) {
            Ok(group) => group,
            Err(_) => continue,
        };
        if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
            continue;
        }
        let mut state = load_state(&store, &group.group_id)?;
        emitted += tick_rules(&store, &group, &mut state)?;
        if group.state == GroupState::Active {
            emitted += tick_unread(home, &store, &group, &mut state)?;
        }
        save_state(&store, &group.group_id, &state)?;
    }
    Ok(emitted)
}

fn tick_rules(store: &GroupStore, group: &GroupDoc, state: &mut RuntimeState) -> io::Result<usize> {
    let Some(rules) = group.automation.get("rules").and_then(Value::as_array) else {
        return Ok(0);
    };
    let now = Utc::now().timestamp();
    let mut emitted = 0;
    for rule in rules.iter().filter_map(Value::as_object) {
        if rule.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let id = rule.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let interval = rule
            .get("interval_seconds")
            .and_then(Value::as_i64)
            .or_else(|| {
                rule.get("interval_minutes")
                    .and_then(Value::as_i64)
                    .map(|value| value * 60)
            })
            .unwrap_or(0);
        if interval <= 0 || now - state.last_rule.get(id).copied().unwrap_or(0) < interval {
            continue;
        }
        let text = rule
            .get("message")
            .or_else(|| rule.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let mut event = Event::new("system.notify", &group.group_id);
        event.by = "system".into();
        event.data = json!({
            "kind": "automation_rule", "rule_id": id, "text": text,
            "to": rule.get("to").cloned().unwrap_or_else(|| json!(["@all"])),
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(&store.ledger_path(&group.group_id)?, &event)?;
        state.last_rule.insert(id.into(), now);
        emitted += 1;
    }
    Ok(emitted)
}

fn tick_unread(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    state: &mut RuntimeState,
) -> io::Result<usize> {
    let threshold = group
        .automation
        .get("unread_nudge_after_seconds")
        .or_else(|| group.automation.get("nudge_after_seconds"))
        .and_then(Value::as_i64)
        .unwrap_or(300);
    if threshold <= 0 {
        return Ok(0);
    }
    let now = Utc::now().timestamp();
    let mut emitted = 0;
    for actor in actors::visible(group).filter(|actor| actor.enabled) {
        let unread = inbox::list_unread(home, group, &actor.id, 1)?;
        let Some(message) = unread.first() else {
            continue;
        };
        let sent = DateTime::parse_from_rfc3339(&message.ts)
            .map(|value| value.timestamp())
            .unwrap_or(now);
        let key = format!("{}:{}", actor.id, message.id);
        if now - sent < threshold
            || now - state.last_nudge.get(&key).copied().unwrap_or(0) < threshold
        {
            continue;
        }
        let mut event = Event::new("system.notify", &group.group_id);
        event.by = "system".into();
        event.data = json!({
            "kind": "unread_nudge", "actor_id": actor.id, "to": [actor.id],
            "event_id": message.id, "text": "You have an unread collaboration message.",
            "created_at": utc_now(),
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(&store.ledger_path(&group.group_id)?, &event)?;
        state.last_nudge.insert(key, now);
        emitted += 1;
    }
    Ok(emitted)
}

fn load_state(store: &GroupStore, group_id: &str) -> io::Result<RuntimeState> {
    let path = store.state_dir(group_id)?.join("automation-runtime.json");
    if path.exists() {
        read_json(&path)
    } else {
        Ok(RuntimeState::default())
    }
}
fn save_state(store: &GroupStore, group_id: &str, state: &RuntimeState) -> io::Result<()> {
    write_json(
        &store.state_dir(group_id)?.join("automation-runtime.json"),
        state,
    )
}

use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::Value;
use std::path::Path;

const RECOVERY_MAX_BYTES: u64 = 4 * 1024 * 1024;
const RECOVERY_MAX_LINES: usize = 16_384;

/// Rebuild the DeepSeek cursor from durable terminal events after a daemon
/// restart. Only a contiguous unread prefix is eligible; no prompt or output
/// is replayed here.
pub fn recover(home: &HomeLayout, group: &GroupDoc, actor: &Actor, limit: usize) -> usize {
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return 0;
    };
    let Ok(path) = store
        .state_dir(&group.group_id)
        .map(|dir| dir.join("headless").join("events.jsonl"))
    else {
        return 0;
    };
    let Some(values) = read_bounded_events(&path) else {
        return 0;
    };
    let completed = values
        .into_iter()
        .filter(|value| {
            value.get("group_id").and_then(Value::as_str) == Some(group.group_id.as_str())
                && value.get("actor_id").and_then(Value::as_str) == Some(actor.id.as_str())
                && value.get("type").and_then(Value::as_str) == Some("headless.turn.completed")
        })
        .filter_map(|value| {
            value
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("event_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<std::collections::HashSet<_>>();
    if completed.is_empty() {
        return 0;
    }
    let Ok(unread) = cccc_core::inbox::list_unread(home, group, &actor.id, limit.max(1), "all")
    else {
        return 0;
    };
    let mut recovered = 0;
    for event in unread {
        if !completed.contains(&event.id) {
            break;
        }
        match cccc_core::inbox::advance(home, &group.group_id, &actor.id, &event.id) {
            Ok(true) => recovered += 1,
            Ok(false) => continue,
            Err(_) => break,
        }
    }
    recovered
}

pub(super) fn read_bounded_events(path: &Path) -> Option<Vec<Value>> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > RECOVERY_MAX_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() > RECOVERY_MAX_LINES {
        return None;
    }
    Some(
        lines
            .into_iter()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect(),
    )
}

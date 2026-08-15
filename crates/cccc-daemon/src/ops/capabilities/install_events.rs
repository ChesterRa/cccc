use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

use crate::dispatch::{OpError, OpResult};

pub(super) struct InstallChange<'a> {
    pub action_id: &'a str,
    pub group_id: &'a str,
    pub actor_id: &'a str,
    pub by: &'a str,
    pub scope: &'a str,
    pub capability_ids: &'a [String],
}

pub(super) fn finish(
    home: &HomeLayout,
    mut result: Map<String, Value>,
    change: &InstallChange<'_>,
) -> OpResult {
    if change.capability_ids.is_empty() {
        return Ok(result);
    }
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("changed");
    if let Err(error) = append_changed_event(home, change, state) {
        result.insert(
            "event_publish_error".into(),
            json!(error.message.chars().take(500).collect::<String>()),
        );
    }
    Ok(result)
}

pub(super) fn records_differ(previous: Option<&Value>, current: Option<&Value>) -> bool {
    previous.map(semantic_record) != current.map(semantic_record)
}

fn append_changed_event(
    home: &HomeLayout,
    change: &InstallChange<'_>,
    state: &str,
) -> Result<(), OpError> {
    let unique_ids = change
        .capability_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if unique_ids.is_empty() {
        return Ok(());
    }
    let capability_id = unique_ids[0].clone();
    let mut event = Event::new("capability.changed", change.group_id);
    event.by = change.by.to_owned();
    event.data = json!({
        "action_id":change.action_id,
        "actor_id":change.actor_id,
        "capability_id":capability_id,
        "capability_ids":unique_ids,
        "scope":change.scope,
        "state":state,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    let path = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .ledger_path(change.group_id)
        .map_err(OpError::io)?;
    ledger::append(&path, &event).map_err(OpError::io)
}

fn semantic_record(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(record) = record.as_object_mut() {
        record.remove("updated_at_source");
        record.remove("last_synced_at");
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_event_deduplicates_a_multi_skill_batch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("groups");
        let group = store.create("multi skill event", "").expect("group");

        let capability_ids = [
            "skill:test:b".into(),
            "skill:test:a".into(),
            "skill:test:b".into(),
        ];
        let change = InstallChange {
            action_id: "cins_test",
            group_id: &group.group_id,
            actor_id: "user",
            by: "user",
            scope: "group",
            capability_ids: &capability_ids,
        };

        append_changed_event(&home, &change, "ready").expect("append event");

        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("events")
            .into_iter()
            .filter(|event| event.kind == "capability.changed")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].data["capability_ids"],
            json!(["skill:test:a", "skill:test:b"])
        );
    }
}

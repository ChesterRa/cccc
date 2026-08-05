use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::dispatch::OpError;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Completion {
    pub turn_id: String,
    pub event_ids: Vec<String>,
    pub status: String,
    pub delivery_id: String,
}

pub(super) fn find(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    completion: &Completion,
) -> Result<Option<Event>, OpError> {
    let path = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .ledger_path(group_id)
        .map_err(OpError::io)?;
    let Some(event) = ledger::find_event(&path, &event_id(group_id, actor_id, &completion.turn_id))
        .map_err(OpError::io)?
    else {
        return Ok(None);
    };
    if matches(&event, actor_id, completion) {
        Ok(Some(event))
    } else {
        Err(OpError::new(
            "completion_conflict",
            "turn completion receipt does not match this request",
        ))
    }
}

pub(super) fn append(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    completion: &Completion,
) -> Result<Event, OpError> {
    if let Some(event) = find(home, group_id, actor_id, completion)? {
        return Ok(event);
    }
    let mut event = Event::new(kind(completion), group_id);
    event.id = event_id(group_id, actor_id, &completion.turn_id);
    event.by = actor_id.into();
    event.data = Map::from_iter([
        ("actor_id".into(), json!(actor_id)),
        (
            "event_id".into(),
            json!(completion.event_ids.last().expect("event ids validated")),
        ),
        ("turn_id".into(), json!(completion.turn_id)),
        ("event_ids".into(), json!(completion.event_ids)),
        ("status".into(), json!(completion.status)),
        ("delivery_id".into(), json!(completion.delivery_id)),
        (
            "cursor_committed".into(),
            json!(cursor_committed(completion)),
        ),
    ]);
    let path = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .ledger_path(group_id)
        .map_err(OpError::io)?;
    ledger::append(&path, &event).map_err(OpError::io)?;
    Ok(event)
}

fn matches(event: &Event, actor_id: &str, completion: &Completion) -> bool {
    event.kind == kind(completion)
        && event.by == actor_id
        && string(&event.data, "actor_id") == Some(actor_id)
        && string(&event.data, "turn_id") == Some(completion.turn_id.as_str())
        && event.data.get("event_ids") == Some(&json!(completion.event_ids))
        && string(&event.data, "status") == Some(completion.status.as_str())
        && string(&event.data, "delivery_id") == Some(completion.delivery_id.as_str())
        && event.data.get("cursor_committed").and_then(Value::as_bool)
            == Some(cursor_committed(completion))
}

fn string<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

fn kind(completion: &Completion) -> &'static str {
    if cursor_committed(completion) {
        "chat.read"
    } else {
        "runtime.turn.completed"
    }
}

fn cursor_committed(completion: &Completion) -> bool {
    matches!(completion.status.as_str(), "done" | "partial")
}

fn event_id(group_id: &str, actor_id: &str, turn_id: &str) -> String {
    let digest = Sha256::digest(format!(
        "runtime-completion\0{group_id}\0{actor_id}\0{turn_id}"
    ));
    format!("{digest:x}")[..32].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_identity_compares_the_full_fingerprint() {
        let exact = Completion {
            turn_id: "turn-a".into(),
            event_ids: vec!["event-a".into()],
            status: "done".into(),
            delivery_id: "delivery-a".into(),
        };
        let mut event = Event::new(kind(&exact), "group-a");
        event.by = "actor-a".into();
        event.data = Map::from_iter([
            ("actor_id".into(), json!("actor-a")),
            ("event_id".into(), json!("event-a")),
            ("turn_id".into(), json!("turn-a")),
            ("event_ids".into(), json!(["event-a"])),
            ("status".into(), json!("done")),
            ("delivery_id".into(), json!("delivery-a")),
            ("cursor_committed".into(), json!(true)),
        ]);
        assert!(matches(&event, "actor-a", &exact));
        let mut mismatch = exact.clone();
        mismatch.delivery_id = "delivery-b".into();
        assert!(!matches(&event, "actor-a", &mismatch));
    }
}

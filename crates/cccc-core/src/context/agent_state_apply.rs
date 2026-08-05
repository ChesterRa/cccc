use cccc_contracts::utc_now;
use serde_json::{Map, Value, json};
use std::io;

use super::model::ContextDoc;

pub(super) fn update(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    let actor = required_actor(op)?;
    let state = doc.agent_states.entry(actor.into()).or_default();
    let hot = nested_object(state, "hot")?;
    for key in ["active_task_id", "focus", "next_action", "blockers"] {
        if let Some(value) = op.get(key) {
            hot.insert(key.into(), value.clone());
        }
    }
    let warm = nested_object(state, "warm")?;
    for (target, aliases) in [
        ("what_changed", &["what_changed"][..]),
        ("open_loops", &["open_loops"][..]),
        ("commitments", &["commitments"][..]),
        (
            "environment_summary",
            &["environment_summary", "environment"][..],
        ),
        ("user_model", &["user_model", "user_profile"][..]),
        ("persona_notes", &["persona_notes", "notes"][..]),
    ] {
        if let Some(value) = aliases.iter().find_map(|key| op.get(*key)) {
            warm.insert(target.into(), value.clone());
        }
    }
    state.insert("updated_at".into(), Value::String(utc_now()));
    Ok(())
}

pub(super) fn clear(doc: &mut ContextDoc, op: &Map<String, Value>) -> io::Result<()> {
    doc.agent_states.insert(
        required_actor(op)?.into(),
        Map::from_iter([
            ("hot".into(), json!({})),
            ("warm".into(), json!({})),
            ("updated_at".into(), Value::String(utc_now())),
        ]),
    );
    Ok(())
}

fn required_actor(op: &Map<String, Value>) -> io::Result<&str> {
    op.get("actor_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| io::Error::other("actor_id is required"))
}

fn nested_object<'a>(
    state: &'a mut Map<String, Value>,
    key: &str,
) -> io::Result<&'a mut Map<String, Value>> {
    state
        .entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| io::Error::other(format!("invalid agent state {key}")))
}

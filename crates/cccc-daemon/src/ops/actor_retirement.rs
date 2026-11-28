//! What a removed Actor leaves behind is settled when it is removed, so no
//! work is left waiting on a member that no longer exists.
//!
//! Its unfinished tasks pass to the Foreman. This runs after the removal is
//! committed: a failure here is logged and never undoes the removal.

use cccc_contracts::Event;
use cccc_core::context::ContextStore;
use cccc_core::{GroupStore, HomeLayout, actors, ledger};
use serde_json::{Map, Value, json};

/// Settles everything the removed Actor left open. `by` is who removed it.
pub(super) fn settle(home: &HomeLayout, group_id: &str, actor_id: &str, by: &str) {
    if let Err(error) = retire_actor_context(home, group_id, actor_id, by) {
        tracing::warn!(%error, %group_id, %actor_id, "post-commit Actor context cleanup failed");
    }
}

/// The member that takes over a removed Actor's unfinished tasks: the Group's
/// Foreman as it is after the removal. `None` when no member is left.
fn heir(home: &HomeLayout, group_id: &str) -> std::io::Result<Option<String>> {
    let group = GroupStore::new(home.clone())?.load(group_id)?;
    Ok(actors::visible(&group).next().map(|actor| actor.id.clone()))
}

/// Hand a removed Actor's unfinished tasks to the Foreman and clear its
/// working state. A task it owned gets the Foreman as assignee; a handoff
/// aimed at it is cleared. Task progress, content, other Actors and finished
/// ownership stay unchanged. One `context.sync` event records the cleanup.
fn retire_actor_context(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    by: &str,
) -> std::io::Result<()> {
    let contexts = ContextStore::new(home.clone())?;
    let heir = heir(home, group_id)?;
    // The version check retries when the board changes between read and write.
    for _ in 0..3 {
        let document = contexts.load(group_id)?;
        let mut operations = document
            .tasks
            .iter()
            .filter(|task| {
                ["assignee", "handoff_to"]
                    .iter()
                    .any(|field| task.get(*field).and_then(Value::as_str) == Some(actor_id))
                    && matches!(
                        task.get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("planned"),
                        "planned" | "active"
                    )
            })
            .filter_map(|task| {
                let task_id = task.get("id").and_then(Value::as_str)?;
                let mut operation = Map::from_iter([
                    ("op".into(), json!("task.update")),
                    ("task_id".into(), json!(task_id)),
                ]);
                if task.get("assignee").and_then(Value::as_str) == Some(actor_id) {
                    operation.insert("assignee".into(), json!(heir));
                }
                if task.get("handoff_to").and_then(Value::as_str) == Some(actor_id) {
                    operation.insert("handoff_to".into(), Value::Null);
                }
                Some(operation)
            })
            .collect::<Vec<_>>();
        if document.agent_states.contains_key(actor_id) {
            operations.push(Map::from_iter([
                ("op".into(), json!("agent_state.clear")),
                ("actor_id".into(), json!(actor_id)),
            ]));
        }
        if operations.is_empty() {
            return Ok(());
        }
        let version = contexts.version(&document)?;
        let result = match contexts.sync(group_id, &operations, Some(&version), by, false) {
            Ok(result) => result,
            Err(error) if error.to_string() == "version_conflict" => continue,
            Err(error) => return Err(error),
        };
        let mut event = Event::new("context.sync", group_id);
        event.by = by.to_owned();
        event.data = Map::from_iter([
            ("version".into(), json!(result.version)),
            ("changes".into(), json!(result.changes)),
        ]);
        return ledger::append(
            &GroupStore::new(home.clone())?.ledger_path(group_id)?,
            &event,
        );
    }
    Err(std::io::Error::other(
        "context kept changing during Actor retirement",
    ))
}

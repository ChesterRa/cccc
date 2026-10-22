//! With the Group delivery setting `task_assigned_notice`, a `context_sync`
//! that names an Actor as a task's assignee or handoff target tells that Actor
//! with one `system.notify(kind="task_assigned")`. The writer is not told about
//! its own writes, and a write that leaves the field as it was tells no one.

use cccc_contracts::{Actor, Event};
use cccc_core::context::ContextDoc;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, ledger};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

use crate::ops::actor_delivery;

const FIELDS: [&str; 2] = ["assignee", "handoff_to"];

fn enabled(group: &GroupDoc) -> bool {
    group
        .extra
        .get("delivery")
        .and_then(Value::as_object)
        .and_then(|delivery| delivery.get("task_assigned_notice"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Write and hand over the notices for one committed sync, `sync` being its
/// `context.sync` event. The write already stands, so a failure here is logged
/// rather than reported to the writer.
pub(super) fn issue(
    home: &HomeLayout,
    group: &GroupDoc,
    sync: &Event,
    before: &ContextDoc,
    after: &ContextDoc,
) {
    if !enabled(group) {
        return;
    }
    let notices = assignments(before, after)
        .into_iter()
        .filter(|(actor_id, _)| *actor_id != sync.by)
        .filter_map(|(actor_id, task_ids)| {
            actors::visible(group)
                .find(|actor| actor.id == actor_id && actor.enabled)
                .map(|actor| notice(group, actor, sync, task_ids))
        })
        .collect::<Vec<_>>();
    if notices.is_empty() {
        return;
    }
    let path = match GroupStore::new(home.clone())
        .and_then(|groups| groups.ledger_path(&group.group_id))
    {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(%error, group_id = %group.group_id, "task assignment notice failed");
            return;
        }
    };
    for event in notices {
        if let Err(error) = ledger::append(&path, &event) {
            tracing::warn!(%error, group_id = %group.group_id, "task assignment notice failed");
            return;
        }
        let _ = actor_delivery::dispatch(home, group, &event);
    }
}

/// The tasks each Actor id was newly named on, as assignee or handoff target,
/// in task order.
fn assignments<'a>(before: &ContextDoc, after: &'a ContextDoc) -> BTreeMap<&'a str, Vec<&'a str>> {
    let previous = before
        .tasks
        .iter()
        .filter_map(|task| Some((task.get("id")?.as_str()?, task)))
        .collect::<HashMap<_, _>>();
    let mut assigned = BTreeMap::<&str, Vec<&str>>::new();
    for task in &after.tasks {
        let Some(task_id) = task.get("id").and_then(Value::as_str) else {
            continue;
        };
        let prior = previous.get(task_id);
        for field in FIELDS {
            let Some(actor_id) = named(task, field) else {
                continue;
            };
            if prior.and_then(|prior| named(prior, field)) == Some(actor_id) {
                continue;
            }
            let task_ids = assigned.entry(actor_id).or_default();
            if task_ids.last() != Some(&task_id) {
                task_ids.push(task_id);
            }
        }
    }
    assigned
}

fn named<'a>(task: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    task.get(field)
        .and_then(Value::as_str)
        .filter(|actor_id| !actor_id.trim().is_empty())
}

fn notice(group: &GroupDoc, actor: &Actor, sync: &Event, task_ids: Vec<&str>) -> Event {
    let mut event = cccc_core::automation::notice_event(
        group,
        actor,
        "task_assigned",
        "Task assigned",
        &format!(
            "{} assigned or handed off {} task(s) to you: {}. Call cccc_task to read them when appropriate.",
            sync.by,
            task_ids.len(),
            task_ids.join(", ")
        ),
        vec![sync.id.clone()],
    );
    if let Some(context) = event.data.get_mut("context").and_then(Value::as_object_mut) {
        context.insert("task_ids".into(), json!(task_ids));
        context.insert("assigned_by".into(), json!(sync.by));
    }
    event
}

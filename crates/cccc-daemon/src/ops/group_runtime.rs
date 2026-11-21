use cccc_contracts::GroupState;
use cccc_core::actors;
use cccc_core::{GroupDoc, registry::GroupMeta};
use serde_json::{Value, json};

use crate::ops::actor_runtime_status;

pub fn status(home: &cccc_core::HomeLayout, group: &GroupDoc) -> Value {
    crate::session_owner::read_scope(|| status_snapshot(home, group))
}

fn status_snapshot(home: &cccc_core::HomeLayout, group: &GroupDoc) -> Value {
    if crate::session_owner::require_available(home, Some(&group.group_id), None).is_err() {
        return json!({"lifecycle_state":group.state,"available":false});
    }
    let running: Vec<_> = group
        .actors
        .iter()
        .filter(|actor| actor.enabled && actor_runtime_status::resolve(home, group, actor).running)
        .collect();
    let lifecycle = if running.is_empty() && group.state == GroupState::Stopped {
        GroupState::Stopped
    } else {
        group.state
    };
    json!({
        "lifecycle_state": lifecycle,
        "available": true,
        "runtime_running": !running.is_empty(),
        "running_actor_count": running.len(),
        "has_running_foreman": running.iter().any(|actor| {
            actors::effective_role(group, &actor.id) == Some(cccc_contracts::ActorRole::Foreman)
        }),
    })
}

pub fn group(home: &cccc_core::HomeLayout, group: GroupDoc) -> Value {
    crate::session_owner::read_scope(|| group_snapshot(home, group))
}

fn group_snapshot(home: &cccc_core::HomeLayout, group: GroupDoc) -> Value {
    let runtime_status = status(home, &group);
    let desired_running = group.running;
    let available = runtime_status["available"].as_bool() != Some(false);
    let mut value = serde_json::to_value(&group).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("desired_running".into(), Value::Bool(desired_running));
        object.insert("runtime_status".into(), runtime_status.clone());
        if available {
            object.insert("running".into(), runtime_status["runtime_running"].clone());
        } else {
            // Configuration writes have already succeeded. Omit observations
            // rather than report a failed write or invent a stopped process.
            object.remove("running");
        }
        if let Some(items) = object.get_mut("actors").and_then(Value::as_array_mut) {
            for (item, actor) in items.iter_mut().zip(&group.actors) {
                item["role"] = serde_json::to_value(actors::effective_role(&group, &actor.id))
                    .unwrap_or(Value::Null);
                let status = actor_runtime_status::resolve(home, &group, actor);
                if status.available {
                    item["running"] = Value::Bool(status.running);
                    item["pid"] = status
                        .pid
                        .map_or(Value::Null, |pid| Value::from(u64::from(pid)));
                } else if let Some(item) = item.as_object_mut() {
                    item.remove("running");
                    item.remove("pid");
                    item.extend(super::working_state::runtime_actor_fields(
                        home,
                        actor,
                        &group.group_id,
                        false,
                    ));
                }
            }
        }
    }
    value
}

pub fn summary(home: &cccc_core::HomeLayout, meta: GroupMeta, group: &GroupDoc) -> Value {
    crate::session_owner::read_scope(|| summary_snapshot(home, meta, group))
}

fn summary_snapshot(home: &cccc_core::HomeLayout, meta: GroupMeta, group: &GroupDoc) -> Value {
    let runtime_status = status(home, group);
    let mut value = serde_json::to_value(meta).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("desired_running".into(), Value::Bool(group.running));
        object.insert("state".into(), json!(group.state));
        object.insert("runtime_status".into(), runtime_status.clone());
        if runtime_status["available"].as_bool() != Some(false) {
            object.insert("running".into(), runtime_status["runtime_running"].clone());
        } else {
            object.remove("running");
        }
    }
    value
}

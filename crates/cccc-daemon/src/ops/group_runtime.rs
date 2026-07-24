use cccc_contracts::GroupState;
use cccc_core::actors;
use cccc_core::{GroupDoc, registry::GroupMeta};
use serde_json::{Value, json};

use crate::ops::actor_runtime;

pub fn status(group: &GroupDoc) -> Value {
    let running: Vec<_> = group
        .actors
        .iter()
        .filter(|actor| {
            actor.enabled
                && (if super::local_headless::supports(actor) {
                    super::local_headless::running(&group.group_id, &actor.id)
                } else {
                    actor_runtime::is_structured(actor)
                        && group.running
                        && group.state != GroupState::Stopped
                        || actor_runtime::status(&group.group_id, &actor.id)
                            .is_some_and(|status| status.running)
                })
        })
        .collect();
    let lifecycle = if running.is_empty() && group.state == GroupState::Stopped {
        GroupState::Stopped
    } else {
        group.state
    };
    json!({
        "lifecycle_state": lifecycle,
        "runtime_running": !running.is_empty(),
        "running_actor_count": running.len(),
        "has_running_foreman": running.iter().any(|actor| {
            actors::effective_role(group, &actor.id) == Some(cccc_contracts::ActorRole::Foreman)
        }),
    })
}

pub fn group(group: GroupDoc) -> Value {
    let runtime_status = status(&group);
    let desired_running = group.running;
    let mut value = serde_json::to_value(&group).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("desired_running".into(), Value::Bool(desired_running));
        object.insert(
            "running".into(),
            Value::Bool(runtime_status["runtime_running"].as_bool().unwrap_or(false)),
        );
        object.insert("runtime_status".into(), runtime_status);
        if let Some(items) = object.get_mut("actors").and_then(Value::as_array_mut) {
            for (item, actor) in items.iter_mut().zip(&group.actors) {
                item["role"] = serde_json::to_value(actors::effective_role(&group, &actor.id))
                    .unwrap_or(Value::Null);
                if actor_runtime::is_structured(actor) {
                    item["running"] = Value::Bool(if super::local_headless::supports(actor) {
                        super::local_headless::running(&group.group_id, &actor.id)
                    } else {
                        actor.enabled && group.running && group.state != GroupState::Stopped
                    });
                    item["pid"] = Value::Null;
                }
            }
        }
    }
    value
}

pub fn summary(meta: GroupMeta, group: &GroupDoc) -> Value {
    let runtime_status = status(group);
    let mut value = serde_json::to_value(meta).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("desired_running".into(), Value::Bool(group.running));
        object.insert(
            "running".into(),
            Value::Bool(runtime_status["runtime_running"].as_bool().unwrap_or(false)),
        );
        object.insert("state".into(), json!(group.state));
        object.insert("runtime_status".into(), runtime_status);
    }
    value
}

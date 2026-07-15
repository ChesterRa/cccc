use cccc_contracts::{DaemonRequest, Event};
use cccc_core::automation::{ScheduledAction, TickResult};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox};
use serde_json::json;

use crate::dispatch::dispatch;
use crate::ops::{actor_delivery, actor_runtime, group_runtime};

pub fn tick(home: &HomeLayout, include_unread: bool) {
    actor_delivery::drain(home);
    if let Err(error) = actor_runtime::reconcile(home) {
        tracing::warn!(message = %error.message, "runtime reconciliation failed");
    }
    match cccc_core::automation::tick_scheduled(home, include_unread) {
        Ok(result) => apply(home, result),
        Err(error) => tracing::warn!(%error, "automation tick failed"),
    }
}

fn apply(home: &HomeLayout, result: TickResult) {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    for event in result.notifications {
        if let Ok(group) = store.load(&event.group_id) {
            actor_delivery::dispatch(home, &group, &event);
        }
    }
    for action in result.actions {
        match action {
            ScheduledAction::GroupState { group_id, state } => {
                let op = if state == "stopped" {
                    "group_stop"
                } else {
                    "group_set_state"
                };
                if state == "active"
                    && store.load(&group_id).is_ok_and(|group| {
                        !group_runtime::status(&group)["runtime_running"]
                            .as_bool()
                            .unwrap_or(false)
                    })
                {
                    call(
                        home,
                        "group_start",
                        json!({"group_id":group_id,"by":"user"}),
                    );
                }
                call(
                    home,
                    op,
                    json!({"group_id":group_id,"state":state,"by":"user"}),
                );
            }
            ScheduledAction::ActorControl {
                group_id,
                operation,
                targets,
            } => {
                let Ok(group) = store.load(&group_id) else {
                    continue;
                };
                let op = match operation.as_str() {
                    "start" => "actor_start",
                    "stop" => "actor_stop",
                    "restart" => "actor_restart",
                    _ => continue,
                };
                for actor_id in matching_actors(&group, &targets) {
                    call(
                        home,
                        op,
                        json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
                    );
                }
            }
        }
    }
}

fn matching_actors(group: &GroupDoc, targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "system".into();
    event.data = json!({"to":targets})
        .as_object()
        .cloned()
        .unwrap_or_default();
    actors::visible(group)
        .filter(|actor| inbox::is_for_actor(group, &event, &actor.id))
        .map(|actor| actor.id.clone())
        .collect()
}

fn call(home: &HomeLayout, op: &str, value: serde_json::Value) {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: value.as_object().cloned().unwrap_or_default(),
    };
    let response = dispatch(home, &request);
    if !response.ok {
        tracing::warn!(%op, "scheduled automation action failed");
    }
}

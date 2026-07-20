use cccc_contracts::{DaemonRequest, Event};
use cccc_core::automation::{ScheduledAction, TickResult};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dispatch::dispatch;
use crate::ops::{actor_delivery, actor_runtime, group_runtime};

pub fn prepare_exited() -> BTreeMap<String, Vec<cccc_runtime::SessionStatus>> {
    let exited = match actor_runtime::reap_exited() {
        Ok(exited) => exited,
        Err(error) => {
            tracing::warn!(message = %error.message, "runtime reap failed");
            Vec::new()
        }
    };
    let mut grouped = BTreeMap::<String, Vec<_>>::new();
    for status in exited {
        grouped
            .entry(status.group_id.clone())
            .or_default()
            .push(status);
    }
    grouped
}

pub fn pending_delivery_group_ids() -> Vec<String> {
    actor_delivery::pending_group_ids()
}

pub fn maintain_group(home: &HomeLayout, group_id: &str, exited: Vec<cccc_runtime::SessionStatus>) {
    actor_delivery::drain_group(home, group_id);
    if let Err(error) = actor_runtime::reconcile_exited(home, exited) {
        tracing::warn!(message = %error.message, %group_id, "runtime reconciliation failed");
    }
}

pub fn group_ids(home: &HomeLayout) -> Vec<String> {
    match cccc_core::automation::group_ids(home) {
        Ok(group_ids) => group_ids,
        Err(error) => {
            tracing::warn!(%error, "automation group discovery failed");
            Vec::new()
        }
    }
}

pub fn tick_group(home: &HomeLayout, group_id: &str, include_unread: bool, cancelled: &AtomicBool) {
    if cancelled.load(Ordering::Acquire) {
        return;
    }
    match cccc_core::automation::tick_group(home, group_id, include_unread) {
        Ok(result) => apply(home, result, cancelled),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, %group_id, "automation group tick failed"),
    }
}

fn apply(home: &HomeLayout, result: TickResult, cancelled: &AtomicBool) {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    for event in result.notifications {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        if let Ok(group) = store.load(&event.group_id) {
            actor_delivery::dispatch(home, &group, &event);
        }
    }
    for action in result.actions {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
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
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    call(
                        home,
                        "group_start",
                        json!({"group_id":group_id,"by":"user"}),
                    );
                }
                if cancelled.load(Ordering::Acquire) {
                    return;
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
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
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

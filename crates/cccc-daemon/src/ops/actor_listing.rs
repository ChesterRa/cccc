use cccc_contracts::{Actor, ActorRuntime, DaemonRequest};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox, ledger};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use crate::dispatch::OpError;

use super::{actor_runtime_status, runtime_session};

pub(super) fn list(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
) -> Result<Vec<Value>, OpError> {
    let include_internal = bool_arg(request, "include_internal");
    let include_unread = bool_arg(request, "include_unread");
    let actor_ids = group
        .actors
        .iter()
        .filter(|actor| include_internal || actor.internal_kind.is_none())
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let unread = if include_unread {
        inbox::list_unread_many(home, group, &actor_ids, 1000)
            .map_err(OpError::io)?
            .into_iter()
            .map(|(actor_id, events)| (actor_id, events.len()))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    group
        .actors
        .iter()
        .filter(|actor| include_internal || actor.internal_kind.is_none())
        .cloned()
        .map(|mut actor| -> Result<Value, OpError> {
            actor.role = actors::effective_role(group, &actor.id);
            let status = actor_runtime_status::resolve(group, &actor);
            let mut value = serde_json::to_value(&actor).unwrap_or_else(|_| json!({}));
            if let Some(object) = value.as_object_mut() {
                object.extend(runtime_session::actor_fields(
                    home,
                    &group.group_id,
                    &actor.id,
                ));
                object.insert("running".into(), Value::Bool(status.running));
                object.insert(
                    "pid".into(),
                    status
                        .pid
                        .map_or(Value::Null, |pid| Value::from(u64::from(pid))),
                );
                object.extend(super::working_state::runtime_actor_fields(
                    home,
                    &actor,
                    &group.group_id,
                    status.running,
                ));
                if actor.runtime == ActorRuntime::WebModel {
                    object.extend(web_model_queue_fields(home, group, &actor)?);
                }
                if include_unread {
                    object.insert(
                        "unread_count".into(),
                        Value::from(
                            u64::try_from(unread.get(&actor.id).copied().unwrap_or_default())
                                .unwrap_or(1000),
                        ),
                    );
                }
            }
            Ok(value)
        })
        .collect()
}

fn web_model_queue_fields(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Result<Map<String, Value>, OpError> {
    let mut fields = Map::from_iter([("web_model_queued_count".into(), Value::from(0))]);
    let state = super::runtime_state::actor_state(home, &group.group_id, &actor.id)?;
    if state.get("status").and_then(Value::as_str) != Some("working") {
        return Ok(fields);
    }
    let active_event_id = state
        .get("latest_event_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active_turn_id = state
        .get("active_turn_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if active_event_id.is_empty() || active_turn_id.is_empty() {
        return Ok(fields);
    }

    let pending = super::runtime_delivery::pending_sources(home, group, actor, 10_000)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let events = ledger::read_all(&store.ledger_path(&group.group_id).map_err(OpError::io)?)
        .map_err(OpError::io)?;
    let Some(active_position) = events.iter().position(|event| event.id == active_event_id) else {
        return Ok(fields);
    };
    let positions = events
        .iter()
        .enumerate()
        .map(|(position, event)| (event.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let queued = pending
        .iter()
        .filter(|event| {
            positions
                .get(event.id.as_str())
                .is_some_and(|position| *position > active_position)
        })
        .collect::<Vec<_>>();
    fields.insert(
        "web_model_queued_count".into(),
        Value::from(u64::try_from(queued.len()).unwrap_or(u64::MAX)),
    );
    if let Some(latest) = queued.last() {
        fields.insert(
            "web_model_queued_after_event_id".into(),
            Value::String(active_event_id.into()),
        );
        fields.insert(
            "web_model_queued_latest_event_id".into(),
            Value::String(latest.id.clone()),
        );
        fields.insert(
            "web_model_queued_latest_ts".into(),
            Value::String(latest.ts.clone()),
        );
    }
    Ok(fields)
}

fn bool_arg(request: &DaemonRequest, name: &str) -> bool {
    request.args.get(name).is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::String(value) => matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"),
        _ => false,
    })
}

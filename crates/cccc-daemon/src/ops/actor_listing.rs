use cccc_contracts::DaemonRequest;
use cccc_core::{GroupDoc, HomeLayout, actors, inbox};
use serde_json::{Value, json};
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
    Ok(group
        .actors
        .iter()
        .filter(|actor| include_internal || actor.internal_kind.is_none())
        .cloned()
        .map(|mut actor| {
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
            value
        })
        .collect())
}

fn bool_arg(request: &DaemonRequest, name: &str) -> bool {
    request.args.get(name).is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::String(value) => matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"),
        _ => false,
    })
}

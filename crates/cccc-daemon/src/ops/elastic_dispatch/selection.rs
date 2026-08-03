use cccc_contracts::{Actor, ActorRole, DaemonRequest};
use cccc_core::{GroupDoc, HomeLayout, actors};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, string_arg};

pub(super) fn authorize_foreman(group: &GroupDoc, by: &str) -> Result<(), OpError> {
    if matches!(by, "user" | "system")
        || actors::effective_role(group, by) == Some(ActorRole::Foreman)
    {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "elastic dispatch is foreman-only",
        ))
    }
}

pub(super) fn select_idle_peer(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
) -> Result<Option<Actor>, OpError> {
    let requested_template = string_arg(request, "template_actor_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let requested_runtime = string_arg(request, "runtime")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let rows = actor_rows(home, group, request)?;
    Ok(rows.into_iter().find_map(|row| {
        let actor_id = row.get("id")?.as_str()?;
        let actor = group.actors.iter().find(|actor| actor.id == actor_id)?;
        let peer = actors::effective_role(group, actor_id) == Some(ActorRole::Peer)
            && actor.internal_kind.is_none();
        let compatible = requested_template
            .as_deref()
            .is_none_or(|wanted| wanted == actor_id)
            && requested_runtime
                .as_deref()
                .is_none_or(|wanted| row.get("runtime").and_then(Value::as_str) == Some(wanted));
        let idle = row.get("running").and_then(Value::as_bool) == Some(true)
            && row.get("effective_working_state").and_then(Value::as_str) == Some("idle")
            && row
                .get("effective_active_task_id")
                .is_none_or(Value::is_null);
        (peer && compatible && idle).then(|| actor.clone())
    }))
}

pub(super) fn actor_row(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    by: &str,
) -> Result<Value, OpError> {
    let request = DaemonRequest {
        v: 1,
        op: "actor_list".into(),
        args: Map::from_iter([
            ("group_id".into(), json!(group.group_id)),
            ("by".into(), json!(by)),
        ]),
    };
    actor_rows(home, group, &request)?
        .into_iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(actor_id))
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))
}

fn actor_rows(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
) -> Result<Vec<Value>, OpError> {
    super::super::actor_listing::list(home, group, request)
}

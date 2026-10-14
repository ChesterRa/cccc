//! Dots owns no browser or local process. The daemon owns one durable wake and
//! the existing Actor turn; callback acknowledgement is not business admission.
use super::operation::{
    Operation,
    Policy::{GlobalWrite, Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, web_model_connectors as store};
use serde_json::{Value, json};
use std::io;

mod delivery;
mod subscription;
#[cfg(test)]
mod tests;
mod work;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "dots_events_status" => Operation::new(Read, status),
        "dots_events_connect" => Operation::new(GlobalWrite, connect),
        "dots_events_disconnect" => Operation::new(GlobalWrite, disconnect),
        "dots_events_subscribe" => Operation::new(GlobalWrite, subscription::subscribe),
        "dots_events_unsubscribe" => Operation::new(GlobalWrite, subscription::unsubscribe),
        "dots_events_next_delivery" => Operation::new(GlobalWrite, delivery::next),
        "dots_events_delivery_record" => Operation::new(GlobalWrite, delivery::record),
        "dots_work_claim" => Operation::new(Write, work::claim),
        "dots_work_control" => Operation::new(Write, work::control),
        _ => return None,
    })
}

// The connector is private YAML. Parser diagnostics can contain secret values.
fn state_error(error: io::Error) -> OpError {
    if error.to_string() == "dots_binding_conflict" {
        return OpError::new(
            "dots_binding_conflict",
            "One Dot can connect to only one Actor",
        );
    }
    OpError::new(
        "dots_state_unavailable",
        "Dots state could not be read or saved",
    )
}

fn denied(code: &'static str) -> io::Error {
    io::Error::other(code)
}

fn user(request: &DaemonRequest) -> Result<(), OpError> {
    if string_arg(request, "by").as_deref() == Some("user") {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "Dots connection controls require the user",
        ))
    }
}

fn group_actor(home: &HomeLayout, request: &DaemonRequest) -> Result<(GroupDoc, Actor), OpError> {
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)?;
    let actor = group
        .actors
        .iter()
        .find(|a| Some(a.id.as_str()) == request.args.get("actor_id").and_then(Value::as_str))
        .cloned()
        .ok_or_else(|| OpError::new("actor_not_found", "Actor not found"))?;
    if actor.runtime != ActorRuntime::ChatgptDots || actor.runner != RunnerKind::Headless {
        return Err(OpError::new(
            "invalid_actor_runtime",
            "Dots requires a ChatGPT Dots Actor",
        ));
    }
    Ok((group, actor))
}

fn active(group: &GroupDoc, actor: &Actor) -> bool {
    actor.enabled
        && group.running
        && !matches!(group.state, GroupState::Paused | GroupState::Stopped)
}

fn binding(connector: &Value, group: &GroupDoc, actor: &Actor) -> Option<Value> {
    (connector["provider"] == "chatgpt_web" && connector["revoked"] != true)
        .then(|| {
            store::binding_for_actor(
                connector,
                &group.group_id,
                &actor.id,
                &cccc_core::actors::generation_identity(actor),
            )
        })
        .flatten()
        .filter(|b| {
            b["transport"] == "mcp_events" && b["revision"] == connector["dots"]["revision"]
        })
}

fn connector(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<Option<Value>, OpError> {
    Ok(store::load(home)
        .map_err(state_error)?
        .into_iter()
        .find(|c| binding(c, group, actor).is_some()))
}

fn subscribed(subscription: &Value, now: i64) -> bool {
    subscription.is_object()
        && subscription["enabled"] == true
        && subscription["subscription_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        && (subscription["expires_at_ms"].is_null()
            || subscription["expires_at_ms"]
                .as_i64()
                .is_some_and(|expiry| expiry > now))
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor) = group_actor(home, request)?;
    object(project(home, &group, &actor)?)
}

pub(super) fn project(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Result<Value, OpError> {
    let connector = connector(home, group, actor)?;
    let dots = connector
        .as_ref()
        .map(|c| &c["dots"])
        .unwrap_or(&Value::Null);
    let runtime = super::runtime_state::actor_state(home, &group.group_id, &actor.id)?;
    let pending = super::runtime_delivery::pending_sources(home, group, actor, 10_000)?.len();
    let has_turn = runtime["active_turn_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty());
    let subscribed = subscribed(&dots["subscription"], chrono::Utc::now().timestamp_millis());
    let paired = connector
        .as_ref()
        .and_then(|c| binding(c, group, actor))
        .is_some_and(|b| b["session_key"].as_str().is_some_and(|key| !key.is_empty()));
    let phase = if has_turn {
        if !active(group, actor)
            || dots["execution"]["state"] != "active"
            || dots["execution"]["turn_id"] != runtime["active_turn_id"]
        {
            "interrupted"
        } else {
            "working"
        }
    } else if connector.is_none() {
        "unconfigured"
    } else if !paired {
        "waiting_pairing"
    } else if !subscribed {
        "waiting_subscription"
    } else if !active(group, actor) {
        "stopped"
    } else {
        match dots["wake"]["state"].as_str() {
            Some("pending") => "pending",
            Some("sending") => "delivering",
            Some("delivered") => "delivered",
            Some("failed") => "failed",
            _ if pending > 0 => "pending",
            _ => "ready",
        }
    };
    let last = &dots["last_execution"];
    Ok(
        json!({"group_id":group.group_id,"actor_id":actor.id,"enabled":actor.enabled,
        "connected":connector.is_some(),"paired":paired,"subscribed":subscribed,"phase":phase,"pending_count":pending,
        "active_turn_id":runtime["active_turn_id"].as_str().unwrap_or(""),
        "active_event_ids":runtime["active_event_ids"].as_array().cloned().unwrap_or_default(),
        "last_execution":if last.is_object() { json!({"turn_id":last["turn_id"],"event_ids":last["event_ids"],
            "status":last["status"],"completed_at":last["completed_at"]}) } else { Value::Null },
        "error_code":if phase == "interrupted" { json!("dots_execution_interrupted") } else { dots["wake"]["error_code"].clone() },
        "subscription_expires_at":dots["subscription"]["expires_at_ms"]}),
    )
}

fn stopped(actor: &Actor) -> Result<(), OpError> {
    if actor.enabled {
        Err(OpError::new(
            "actor_must_be_stopped",
            "Stop the Dots Actor before changing its connection",
        ))
    } else {
        Ok(())
    }
}

fn unresolved(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> Result<(), OpError> {
    let runtime = super::runtime_state::actor_state(home, &group.group_id, &actor.id)?;
    if runtime["active_turn_id"]
        .as_str()
        .is_some_and(|id| !id.is_empty())
    {
        Err(OpError::new(
            "delivery_unresolved",
            "Resolve the unfinished Dots turn before changing its connection",
        ))
    } else {
        Ok(())
    }
}

fn connect(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let (group, actor) = group_actor(home, request)?;
    if super::actor_runtime::actor_is_running(&group, &actor) {
        return Err(OpError::new(
            "actor_must_be_stopped",
            "Stop the previous Actor runtime before connecting Dots",
        ));
    }
    unresolved(home, &group, &actor)?;
    let connectors = store::load(home).map_err(state_error)?;
    let configured = connectors
        .iter()
        .find(|c| c["provider"] == "chatgpt_web" && c["revoked"] != true)
        .ok_or_else(|| {
            OpError::new(
                "connector_unavailable",
                "Configure the shared ChatGPT connector first",
            )
        })?;
    let id = configured["connector_id"]
        .as_str()
        .ok_or_else(|| OpError::new("connector_unavailable", "ChatGPT connector is unavailable"))?;
    if configured["dots"].is_object() && binding(configured, &group, &actor).is_none() {
        return Err(OpError::new(
            "dots_binding_conflict",
            "One Dot can connect to only one Actor",
        ));
    }
    let route = store::bind_dots(
        home,
        id,
        &group.group_id,
        &actor.id,
        &cccc_core::actors::generation_identity(&actor),
    )
    .map_err(state_error)?;
    let current = connector(home, &group, &actor)?
        .ok_or_else(|| OpError::new("connector_unavailable", "Dots connection is unavailable"))?;
    let token = store::dots_subscription_token(&current, &route).map_err(state_error)?;
    let pairing = store::begin_dots_pairing(home, id).map_err(state_error)?;
    object(
        json!({"connected":true,"url":store::DOTS_URL,"subscription_token":token,
        "pairing_code":pairing.as_ref().map(|pair| &pair["code"])}),
    )
}

/// Called only by the authenticated private Web port's existing pairing tool,
/// under the global lifecycle permit. A Dot code pairs only its fixed route.
pub(super) fn accept_pairing(home: &HomeLayout, id: &str, pair: &Value) -> OpResult {
    let request = DaemonRequest {
        v: 1,
        op: "web_model_pairing_accept".into(),
        args: json!({"group_id":pair["group_id"],"actor_id":pair["actor_id"]})
            .as_object()
            .expect("pairing target")
            .clone(),
    };
    let (group, actor) = group_actor(home, &request)?;
    if pair["generation"] != cccc_core::actors::generation_identity(&actor) {
        return Err(OpError::new(
            "pairing_changed",
            "The selected Dot Actor changed; get new setup instructions",
        ));
    }
    if super::actor_runtime::actor_is_running(&group, &actor) {
        return Err(OpError::new(
            "actor_must_be_stopped",
            "Stop the previous Actor runtime before pairing Dots",
        ));
    }
    if pair["state"] != "bound" {
        unresolved(home, &group, &actor)?;
    }
    store::confirm_dots_pairing(home, id, pair["pairing_id"].as_str().unwrap_or_default())
        .map_err(OpError::io)?;
    object(
        json!({"state":"bound","routing_mode":"mcp_events","pairing_id":pair["pairing_id"],
        "group_id":group.group_id,"actor_id":actor.id,"receipt":pair["receipt"]}),
    )
}

fn disconnect(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let (group, actor) = group_actor(home, request)?;
    stopped(&actor)?;
    unresolved(home, &group, &actor)?;
    store::retire_actor(home, &group.group_id, &actor.id).map_err(state_error)?;
    object(json!({"connected":false}))
}

pub(super) fn invalidate(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
) -> Result<(), OpError> {
    if !group
        .actors
        .iter()
        .any(|actor| actor.id == actor_id && actor.runtime == ActorRuntime::ChatgptDots)
    {
        return Ok(());
    }
    store::dots_invalidate(home, &group.group_id, actor_id).map_err(state_error)
}

pub(super) fn invalidate_group(home: &HomeLayout, group: &GroupDoc) -> Result<(), OpError> {
    for actor in &group.actors {
        invalidate(home, group, &actor.id)?;
    }
    Ok(())
}

pub(super) use work::record_completed;

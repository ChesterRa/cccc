//! Native Grok Routine wakes. HTTP acceptance never accepts a business turn.
use super::operation::{
    Operation,
    Policy::{GlobalWrite, Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, web_model_connectors as store};
use serde_json::{Value, json};
use store::grok_webhook as private;

mod delivery;
#[cfg(test)]
mod tests;
mod work;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "grok_webhook_status" => Operation::new(Read, status),
        "grok_webhook_configure" => Operation::new(GlobalWrite, configure),
        "grok_webhook_retry" => Operation::new(Write, retry),
        "grok_webhook_next_delivery" => Operation::new(GlobalWrite, delivery::next),
        "grok_webhook_delivery_record" => Operation::new(GlobalWrite, delivery::record),
        "grok_work_claim" => Operation::new(Write, work::claim),
        "grok_work_resolve" => Operation::new(Write, work::resolve),
        _ => return None,
    })
}
fn state_error(error: std::io::Error) -> OpError {
    match error.to_string().as_str() {
        "grok_webhook_conflict" => {
            return OpError::new(
                "grok_webhook_conflict",
                "This Routine is already configured for another Actor",
            );
        }
        "grok_webhook_key_required" | "invalid_grok_webhook_key" => {
            return OpError::new(
                "invalid_grok_webhook_key",
                "Enter the Key from the native Bot Routine",
            );
        }
        _ => {}
    }
    OpError::new(
        "grok_webhook_state_unavailable",
        "Grok Webhook state could not be read or saved",
    )
}
fn user(r: &DaemonRequest) -> Result<(), OpError> {
    if string_arg(r, "by").as_deref() == Some("user") {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "Grok connection controls require the user",
        ))
    }
}
fn group_actor(home: &HomeLayout, r: &DaemonRequest) -> Result<(GroupDoc, Actor), OpError> {
    let g = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&required_arg(r, "group_id")?)
        .map_err(OpError::not_found)?;
    let a = g
        .actors
        .iter()
        .find(|a| Some(a.id.as_str()) == r.args.get("actor_id").and_then(Value::as_str))
        .filter(|a| a.runtime == ActorRuntime::GrokWebModel && a.runner == RunnerKind::Headless)
        .cloned()
        .ok_or_else(|| {
            OpError::new(
                "invalid_actor_runtime",
                "A Grok Bot headless Actor is required",
            )
        })?;
    Ok((g, a))
}
fn active(g: &GroupDoc, a: &Actor) -> bool {
    a.enabled && g.running && !matches!(g.state, GroupState::Paused | GroupState::Stopped)
}
fn connector(home: &HomeLayout, g: &GroupDoc, a: &Actor) -> Result<Option<Value>, OpError> {
    Ok(store::load(home)
        .map_err(state_error)?
        .into_iter()
        .find(|c| {
            c["provider"] == "grok_web"
                && store::binding_for_actor(
                    c,
                    &g.group_id,
                    &a.id,
                    &cccc_core::actors::generation_identity(a),
                )
                .is_some()
        }))
}
fn status(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    let (g, a) = group_actor(home, r)?;
    object(project(home, &g, &a)?)
}
fn project(home: &HomeLayout, g: &GroupDoc, a: &Actor) -> Result<Value, OpError> {
    let c = store::load(home)
        .map_err(state_error)?
        .into_iter()
        .find(|c| c["provider"] == "grok_web" && c["revoked"] != true)
        .unwrap_or(Value::Null);
    let s = private::state(&c, &g.group_id, &a.id);
    let b = store::binding_for_actor(
        &c,
        &g.group_id,
        &a.id,
        &cccc_core::actors::generation_identity(a),
    )
    .unwrap_or(Value::Null);
    let runtime = super::runtime_state::actor_state(home, &g.group_id, &a.id)?;
    let has_turn = runtime["active_turn_id"]
        .as_str()
        .is_some_and(|s| !s.is_empty());
    let configured = b.is_object() && s["url"].is_string() && s["key"].is_string();
    let pending = super::runtime_delivery::pending_sources(home, g, a, 10_000)?.len();
    let phase = if has_turn {
        if configured
            && active(g, a)
            && s["execution"]["state"] == "active"
            && s["execution"]["turn_id"] == runtime["active_turn_id"]
        {
            "working"
        } else {
            "interrupted"
        }
    } else if !configured {
        "unconfigured"
    } else if !active(g, a) {
        "stopped"
    } else {
        match s["wake"]["state"].as_str() {
            Some("sending") => "delivering",
            Some("pending") => "pending",
            Some("failed") => "failed",
            Some("delivered") => {
                if s["wake"]["sent_at_ms"].as_i64().is_some_and(|t| {
                    chrono::Utc::now().timestamp_millis().saturating_sub(t) >= 120_000
                }) {
                    "unclaimed"
                } else {
                    "delivered"
                }
            }
            _ if pending > 0 => "pending",
            _ => "ready",
        }
    };
    Ok(
        json!({"group_id":g.group_id,"actor_id":a.id,"connected":c.is_object(),"bound":b.is_object(),
        "configured":configured,"phase":phase,"enabled":a.enabled,
        "pending_count":pending,"bot_url":b["url"],"active_turn_id":runtime["active_turn_id"],
        "active_event_ids":runtime["active_event_ids"],"error_code":s["wake"]["error_code"],
        "http_status":s["wake"]["http_status"],"attempt":s["wake"]["attempt"],"last_execution":s["last_execution"]}),
    )
}
fn configure(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    user(r)?;
    let (g, a) = group_actor(home, r)?;
    // GlobalWrite fences claims through validation and the private configuration
    // write. Native Actors need no process restart; preserve their enabled state.
    if super::runtime_state::actor_state(home, &g.group_id, &a.id)?["active_turn_id"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        return Err(OpError::new(
            "delivery_unresolved",
            "Resolve the unfinished turn before changing delivery",
        ));
    }
    let c = connector(home, &g, &a)?
        .ok_or_else(|| OpError::new("grok_binding_required", "Save this Actor's Bot URL first"))?;
    if r.args.contains_key("transport") {
        return Err(OpError::new(
            "unsupported_field",
            "Grok uses native Webhook delivery only",
        ));
    }
    let url = required_arg(r, "url")?;
    let key = required_arg(r, "key")?;
    private::endpoint(&url).map_err(|_| {
        OpError::new(
            "invalid_grok_webhook_url",
            "Use the POST to URL from the desktop Bot Routine",
        )
    })?;
    private::configure(
        home,
        c["connector_id"].as_str().unwrap_or(""),
        &g.group_id,
        &a.id,
        &cccc_core::actors::generation_identity(&a),
        &url,
        &key,
    )
    .map_err(state_error)?;
    super::runtime_delivery::settle_replaced_transport_claims(home, &g, &a.id, "web_model_pull")?;
    status(home, r)
}
fn retry(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    user(r)?;
    let (g, a) = group_actor(home, r)?;
    let p = project(home, &g, &a)?;
    if !active(&g, &a) || !matches!(p["phase"].as_str(), Some("failed" | "unclaimed")) {
        return Err(OpError::new(
            "grok_retry_unavailable",
            "Only a failed or unclaimed wake can be retried; accepted work is never replayed",
        ));
    }
    let c = connector(home, &g, &a)?
        .ok_or_else(|| OpError::new("grok_binding_required", "Grok binding is unavailable"))?;
    private::change(
        home,
        c["connector_id"].as_str().unwrap_or(""),
        &g.group_id,
        &a.id,
        |_, s| {
            s["wake"]["state"] = json!("pending");
            s["wake"]["attempt"] = json!(0);
            s["wake"]["retry_at_ms"] = Value::Null;
            Ok(())
        },
    )
    .map_err(state_error)?;
    status(home, r)
}

pub(super) use work::completed;

pub(super) fn invalidate(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
) -> Result<(), OpError> {
    if group
        .actors
        .iter()
        .any(|a| a.id == actor_id && a.runtime == ActorRuntime::GrokWebModel)
    {
        private::invalidate(home, &group.group_id, actor_id).map_err(state_error)?;
    }
    Ok(())
}

pub(super) fn invalidate_group(home: &HomeLayout, group: &GroupDoc) -> Result<(), OpError> {
    for actor in &group.actors {
        invalidate(home, group, &actor.id)?;
    }
    Ok(())
}

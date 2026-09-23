//! User-controlled Web Model entrance and conversation binding. The daemon's
//! global write permit serializes these changes with Actor lifecycle changes.
use super::operation::{Operation, Policy::GlobalWrite};
use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use cccc_contracts::{ActorRuntime, DaemonRequest, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, integration_state, web_model_connectors as store};
use serde_json::json;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    matches!(
        request.op.as_str(),
        "web_model_connector_configure"
            | "web_model_connector_revoke"
            | "web_model_pairing_begin"
            | "web_model_pairing_accept"
            | "web_model_pairing_confirm"
            | "web_model_pairing_cancel"
            | "web_model_pairing_fail"
            | "web_model_binding_remove"
    )
    .then(|| Operation::new(GlobalWrite, execute))
}

fn execute(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    // These operations are not exposed by the Actor MCP catalog. The private
    // Web port authenticates the entrance before forwarding accept requests.
    if string_arg(request, "by").as_deref() != Some("user") {
        return Err(OpError::new(
            "permission_denied",
            "Web Model pairing is user-controlled",
        ));
    }
    if request.op == "web_model_connector_configure" {
        return object(store::configure(home).map_err(OpError::io)?);
    }
    let id = required_arg(request, "connector_id")?;
    if request.op == "web_model_connector_revoke" {
        return object(json!({"revoked":store::revoke(home, &id).map_err(OpError::io)?}));
    }
    if request.op == "web_model_pairing_accept" {
        let code = required_arg(request, "code")?;
        let session = required_arg(request, "session_key")?;
        // Candidate receipt grants no tools. Confirmation checks the Actor's
        // current generation and stopped state before granting any authority.
        let pair = store::accept_pairing(home, &id, &code, &session).map_err(OpError::io)?;
        return object(
            json!({"state":pair["state"],"pairing_id":pair["pairing_id"],"receipt":pair["receipt"]}),
        );
    }
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = groups.load(&group_id).map_err(OpError::not_found)?;
    let actor = group
        .actors
        .iter()
        .find(|a| a.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "Actor not found"))?;
    if actor.runtime != ActorRuntime::WebModel || actor.runner != RunnerKind::Headless {
        return Err(OpError::new(
            "invalid_actor_runtime",
            "Pairing requires a Web Model Actor",
        ));
    }
    if request.op == "web_model_pairing_cancel" {
        let pairing_id = required_arg(request, "pairing_id")?;
        store::cancel_pairing(home, &id, &group_id, &actor_id, &pairing_id).map_err(OpError::io)?;
        return object(json!({"cancelled":true}));
    }
    if request.op == "web_model_pairing_fail" {
        let pair = required_arg(request, "pairing_id")?;
        let reason = required_arg(request, "error_code")?;
        if !matches!(
            reason.as_str(),
            "pairing_interrupted"
                | "pairing_timeout"
                | "pairing_target_changed"
                | "pairing_submission_uncertain"
                | "pairing_composer_occupied"
                | "pairing_failed"
        ) {
            return Err(OpError::new("invalid_argument", "Invalid pairing failure"));
        }
        store::fail_pairing(home, &id, &group_id, &actor_id, &pair, &reason)
            .map_err(OpError::io)?;
        return object(json!({"state":"failed","error_code":reason}));
    }
    let connector = store::load(home)
        .map_err(OpError::io)?
        .into_iter()
        .find(|c| c["connector_id"] == id && c["revoked"] != true)
        .ok_or_else(|| {
            OpError::new(
                "connector_unavailable",
                "Configure the shared connector first",
            )
        })?;
    let automatic = if request.op == "web_model_pairing_confirm" {
        store::pairing_for_actor(&connector, &group_id, &actor_id)
            .is_some_and(|p| p["automatic"] == true)
    } else {
        request.op == "web_model_pairing_begin"
            && request.args.get("automatic") == Some(&json!(true))
    };
    if automatic {
        if !actor.enabled
            || !group.running
            || matches!(group.state, GroupState::Paused | GroupState::Stopped)
        {
            return Err(OpError::new(
                "actor_not_active",
                "Automatic connection requires an enabled Actor in a running Group",
            ));
        }
        if store::binding_for_actor(
            &connector,
            &group_id,
            &actor_id,
            &cccc_core::actors::generation_identity(actor),
        )
        .is_some_and(|b| {
            request.op != "web_model_pairing_confirm"
                || request.args.get("pairing_id") != Some(&b["pairing_id"])
        }) {
            return Err(OpError::new(
                "already_paired",
                "Automatic connection cannot replace an existing binding",
            ));
        }
    } else if actor.enabled {
        return Err(OpError::new(
            "actor_must_be_stopped",
            "Stop this Actor before changing its conversation pairing",
        ));
    }
    let runtime = super::runtime_state::actor_state(home, &group_id, &actor_id)?;
    let targets = integration_state::group_get(&groups, &group_id, "web_model_browser_targets")
        .map_err(OpError::io)?;
    let status = targets[&actor_id]["last_delivery_status"]
        .as_str()
        .unwrap_or("");
    let unresolved = runtime["status"] == "working"
        || matches!(
            status,
            "submitting"
                | "deferred"
                | "pending_new_chat_bind"
                | "legacy_recovery_submitting"
                | "submission_ambiguous"
                | "submission_ambiguous_completion_pending"
                | "completion_ambiguous"
                | "ambiguous"
                | "completion_conflict"
                | "legacy_submission_unverified"
        );
    let old_url = targets[&actor_id]["url"]
        .as_str()
        .and_then(|u| store::conversation_url(u).ok());
    let same_url = string_arg(request, "url")
        .and_then(|u| store::conversation_url(&u).ok())
        .is_some_and(|u| Some(u) == old_url);
    // Re-pairing the SAME saved conversation restores access without deleting
    // unresolved evidence. A different conversation still requires resolution.
    if unresolved
        && (automatic
            || !(request.op == "web_model_pairing_begin" && old_url.is_some()
                || request.op == "web_model_pairing_confirm" && same_url))
    {
        return Err(OpError::new(
            "delivery_unresolved",
            if old_url.is_none() {
                "This Actor has an unresolved delivery without a stable conversation URL. Review its old chat, then remove and recreate the stopped Actor to pair a new conversation; Group ledger history is retained."
            } else {
                "Re-pair the same saved conversation and review its pending delivery before changing to another conversation"
            },
        ));
    }
    let generation = cccc_core::actors::generation_identity(actor);
    match request.op.as_str() {
        "web_model_pairing_begin" => object(
            store::begin_pairing(home, &id, &group_id, &actor_id, &generation, automatic)
                .map_err(OpError::io)?,
        ),
        "web_model_pairing_confirm" => {
            let pair = required_arg(request, "pairing_id")?;
            let url = required_arg(request, "url")?;
            let binding =
                store::confirm_pairing(home, &id, &group_id, &actor_id, &generation, &pair, &url)
                    .map_err(OpError::io)?;
            object(json!({"state":"bound","url":binding["url"],"revision":binding["revision"]}))
        }
        "web_model_binding_remove" => {
            store::retire_actor(home, &group_id, &actor_id).map_err(OpError::io)?;
            object(json!({"removed":true}))
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests;

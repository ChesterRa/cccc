//! Confirmation observation and the managed Claude rollback readiness contract.
use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, local_headless};
use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::SessionStatus;
use std::sync::Once;

pub(in crate::ops) const AUTOMATIC_LIFECYCLE: &str = "_cccc_automatic_lifecycle";

pub(in crate::ops) fn user_confirmation_retry(request: &cccc_contracts::DaemonRequest) -> bool {
    request
        .args
        .get(AUTOMATIC_LIFECYCLE)
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        && request
            .args
            .get("by")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("user")
            == "user"
}

/// A recovery-owned terminal is interactive, but it is not a ready managed session.
pub(in crate::ops) fn confirmation_required(group_id: &str, actor_id: &str) -> bool {
    local_headless::waiting_for_workspace_trust(group_id, actor_id).unwrap_or(false)
        && !local_headless::running(group_id, actor_id)
        && super::status(group_id, actor_id).is_some_and(|status| status.running)
}

/// Match the automatic-delivery pause against the Actor's current effective runtime.
pub(in crate::ops) fn confirmation_pause_applies(
    home: &HomeLayout,
    group_id: &str,
    actor: &Actor,
) -> Result<bool, OpError> {
    let paused = local_headless::confirmation_paused(group_id, &actor.id).map_err(|error| {
        // A poisoned registry remains unusable until restart. Actor lists may be
        // polled frequently, so report that one persistent failure only once.
        static REGISTRY_WARNING: Once = Once::new();
        REGISTRY_WARNING.call_once(|| {
            tracing::warn!(kind = ?error.kind(), "confirmation pause registry is unavailable");
        });
        OpError::io(error)
    })?;
    if !paused {
        return Ok(false);
    }
    let effective = actor_profile_runtime::resolve(home, actor)?;
    Ok(effective.runtime == ActorRuntime::Claude && local_headless::supports(&effective))
}

pub(in crate::ops) fn actor_response(
    home: &HomeLayout,
    group_id: &str,
    actor: &Actor,
) -> serde_json::Value {
    let mut value = serde_json::json!(actor);
    value["runtime_confirmation_required"] =
        serde_json::json!(confirmation_required(group_id, &actor.id));
    match confirmation_pause_applies(home, group_id, actor) {
        Ok(paused) => value["runtime_confirmation_paused"] = serde_json::json!(paused),
        Err(error) => tracing::debug!(group_id, actor_id = %actor.id, code = %error.code,
            "could not observe runtime confirmation pause"),
    }
    value
}

/// Keep an interactive recovery terminal open, while reporting an unfinished rollback honestly.
pub(in crate::ops) fn restore_runtime(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
) -> Result<Option<SessionStatus>, OpError> {
    let stored_actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("not_found", format!("actor not found: {actor_id}")))?;
    if !stored_actor.external_session_id.is_empty() {
        return super::apply(home, group, actor_id, "actor.start");
    }
    let actor = actor_profile_runtime::resolve(home, stored_actor)?;
    let status = super::apply_resolved(home, group, &actor, "actor.start")?;
    require_restored_managed_actor(&group.group_id, &actor)?;
    Ok(status)
}

/// A vanished confirmation PTY is not evidence that the previous Claude owner was restored.
pub(in crate::ops) fn require_restored_managed_actor(
    group_id: &str,
    actor: &Actor,
) -> Result<(), OpError> {
    if actor.runtime == ActorRuntime::Claude
        && local_headless::uses_managed_session(actor)
        && !local_headless::running(group_id, &actor.id)
    {
        let pending = confirmation_required(group_id, &actor.id);
        let mut error = OpError::new(
            "runtime_restore_incomplete",
            if pending {
                "Previous agent runtime is waiting for confirmation in its terminal"
            } else {
                "Previous managed Claude runtime did not attach after restart"
            },
        );
        error.details.insert(
            "runtime_confirmation_required".into(),
            serde_json::json!(pending),
        );
        return Err(error);
    }
    Ok(())
}

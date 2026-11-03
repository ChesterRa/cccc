//! Confirmation observation and the managed Claude rollback readiness contract.
use crate::dispatch::OpError;
use crate::ops::{actor_profile_runtime, local_headless};
use cccc_contracts::{Actor, ActorRuntime};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::SessionStatus;

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

pub(in crate::ops) fn actor_response(group_id: &str, actor: &Actor) -> serde_json::Value {
    let mut value = serde_json::json!(actor);
    value["runtime_confirmation_required"] =
        serde_json::json!(confirmation_required(group_id, &actor.id));
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
                "Previous Actor runtime is waiting for confirmation in its terminal"
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

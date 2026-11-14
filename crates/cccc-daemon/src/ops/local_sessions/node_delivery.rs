//! Submit only through the local native adapter after the connector's authority
//! and exact membership snapshot have been checked. Unknown attempts stay fenced.
use super::{codex, transport};
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{
    DaemonRequest,
    session_node::{RemoteSessionDelivery, SessionNodeDeliveryOutcome as Outcome},
};
use cccc_core::{HomeLayout, local_sessions, session_nodes};
use journal::{begin, finish};
use serde_json::json;
use std::sync::atomic::AtomicBool;

mod journal;
#[cfg(all(test, unix))]
mod retention_tests;
#[cfg(test)]
mod tests;

pub(super) fn receive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    super::local(request)?;
    let secret = required_arg(request, "secret")?;
    let delivery: RemoteSessionDelivery =
        serde_json::from_value(request.args.get("delivery").cloned().unwrap_or_default())
            .map_err(OpError::invalid)?;
    let client = session_nodes::client(home)
        .map_err(OpError::io)?
        .filter(|client| client.secret == secret)
        .ok_or_else(denied)?;
    if delivery.handoff_id.is_empty()
        || delivery.handoff_id.len() > 128
        || delivery.text.len() > 512 * 1024
        || delivery.source_id != delivery.membership.source_id
        || delivery.source_revision != delivery.membership.source_revision
        || !delivery.membership.enabled
        || !client.memberships.contains(&delivery.membership)
    {
        return Err(denied());
    }
    let session =
        local_sessions::validate_source(home, &delivery.source_id, &delivery.source_revision)
            .map_err(OpError::io)?;
    if !matches!(
        session.runtime,
        cccc_contracts::local_session::LocalSessionRuntime::Codex
            | cccc_contracts::local_session::LocalSessionRuntime::Claude
    ) {
        return Err(denied());
    }
    let identity = format!(
        "{}:{}:{}",
        client.endpoint.endpoint_id, client.grant_id, delivery.handoff_id
    );
    let fingerprint = serde_json::to_string(&delivery).map_err(OpError::invalid)?;
    if let Some(outcome) = begin(home, &identity, &fingerprint).map_err(OpError::io)? {
        return object(json!({"outcome":outcome}));
    }
    let outcome = if session.runtime == cccc_contracts::local_session::LocalSessionRuntime::Codex {
        codex::submit(
            home,
            &delivery.source_id,
            &delivery.text,
            &AtomicBool::new(false),
        )
    } else {
        transport::submit(
            home,
            transport::Handoff {
                handoff_id: delivery.handoff_id.clone(),
                source_id: delivery.source_id.clone(),
                native_session_id: session.native_session_id,
                group_id: delivery.membership.group_id.clone(),
                actor_id: delivery.membership.actor_id.clone(),
                actor_generation: delivery.membership.actor_generation.clone(),
                remote_membership: Some(delivery.membership),
                event_ids: delivery.event_ids,
                text: delivery.text,
            },
            &AtomicBool::new(false),
        )
    };
    let outcome = match outcome {
        transport::Outcome::Accepted => Outcome::Accepted,
        transport::Outcome::Rejected(reason) => Outcome::Rejected { reason },
        transport::Outcome::Ambiguous(reason) => Outcome::Ambiguous { reason },
        transport::Outcome::Unavailable => Outcome::Unavailable,
    };
    finish(home, &identity, &fingerprint, &outcome).map_err(OpError::io)?;
    object(json!({"outcome":outcome}))
}

fn denied() -> OpError {
    OpError::new(
        "permission_denied",
        "Remote connector or native membership is no longer valid",
    )
}

pub(in crate::ops) fn preflight_event(
    actor: &cccc_contracts::Actor,
    event: &cccc_contracts::Event,
) -> Result<(), super::PreflightError> {
    if session_nodes::is_remote_source(&actor.external_session_id)
        && event
            .data
            .get("attachments")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|files| !files.is_empty())
    {
        return Err(super::PreflightError::Rejected("Device-session delivery currently supports text only; attachments were not transferred or submitted to the native session".into()));
    }
    Ok(())
}

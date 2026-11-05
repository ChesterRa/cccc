//! Completion of a committed switch: the audit event, the handoff notice and
//! the private-env cleanup, each written at most once, then the pending mark is
//! cleared. It runs right after the commit, at daemon start before IPC opens,
//! and on an explicit retry; other changes to the member wait for it.
use cccc_contracts::{Actor, Event};
use cccc_core::runtime_switches::{self, Endpoint, SwitchRecord};
use cccc_core::{GroupDoc, HomeLayout, actors, group_scope, ledger};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::dispatch::{OpError, store};
use crate::ops::{actor_secrets, runtime_delivery};

/// Unconfirmed handoffs listed in the notice text. Native runtimes receive the
/// text as a command argument, so it stays bounded; the notice data keeps the
/// full list.
const LISTED: usize = 20;

pub(crate) struct Completed {
    pub(super) record: SwitchRecord,
    pub(super) notice: Option<Event>,
}

/// Complete the member's unfinished switch, if it has one.
pub(super) fn complete_pending(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<Option<Completed>, OpError> {
    let store = store(home)?;
    let group = store.load(group_id).map_err(OpError::not_found)?;
    let Some(actor) = actors::find(&group, actor_id) else {
        return Ok(None);
    };
    let Some(record) = runtime_switches::pending_for(&group, actor) else {
        return Ok(None);
    };
    // The next owner must not inherit the previous one's pairing. Repeating this
    // on a retry finds nothing left to retire.
    if record.from.runtime != record.to.runtime {
        cccc_core::web_model_connectors::retire_actor(home, group_id, actor_id)
            .map_err(OpError::io)?;
    }
    let ledger_path = store.ledger_path(group_id).map_err(OpError::io)?;
    let scope_key = group_scope::effective_scope(&group, Some(actor))
        .map(|scope| scope.scope_key.clone())
        .unwrap_or_default();
    let audit_id = derived_id(&record.op_id, "audit");
    if ledger::find_event(&ledger_path, &audit_id)
        .map_err(OpError::io)?
        .is_none()
    {
        let mut event = Event::new("actor.update", group_id);
        event.id = audit_id;
        event.by = "user".into();
        event.scope_key.clone_from(&scope_key);
        event.data = json!({
            "actor_id": actor.id,
            "reason": "runtime_switched",
            "op_id": record.op_id,
            "from": record.from,
            "to": record.to,
            "patch": {
                "runtime": actor.runtime,
                "external_session_id": actor.external_session_id,
                "profile_id": actor.profile_id,
                "title": actor.title,
                "default_scope_key": actor.default_scope_key,
                "enabled": actor.enabled,
            },
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(&ledger_path, &event).map_err(OpError::io)?;
    }
    let notice_id = derived_id(&record.op_id, "notice");
    let notice = match ledger::find_event(&ledger_path, &notice_id).map_err(OpError::io)? {
        Some(event) => event,
        None => {
            let unconfirmed =
                runtime_delivery::settle_replaced_owner_claims(home, &group, actor_id)?;
            let mut event = notice(
                group_id,
                actor,
                &record,
                &notice_id,
                &describe(&ledger_path, &unconfirmed)?,
            );
            event.id = notice_id;
            event.scope_key = scope_key;
            ledger::append(&ledger_path, &event).map_err(OpError::io)?;
            event
        }
    };
    if record.clear_private_env {
        actor_secrets::remove(home, group_id, actor_id)?;
    }
    store
        .mutate(group_id, |doc| {
            let mut records = runtime_switches::records(doc);
            records.retain_mut(|stored| {
                if stored.op_id != record.op_id {
                    return true;
                }
                stored.pending = false;
                stored.clear_private_env = false;
                // A source the member left keeps telling that session it was
                // replaced; other records have done their work.
                stored.left_native()
            });
            runtime_switches::set_records(doc, &records);
            Ok(())
        })
        .map_err(OpError::io)?;
    Ok(Some(Completed {
        record,
        notice: Some(notice),
    }))
}

/// Changes that would leave the member's delivery or start to chance wait for
/// the explicit retry, which finishes the switch and resumes the member.
pub(crate) fn refuse_pending(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<(), OpError> {
    let group = store(home)?.load(group_id).map_err(OpError::not_found)?;
    if runtime_switches::has_pending(&group, actor_id) {
        return Err(pending_error(OpError::new(
            "actor_switch_pending",
            "completion has not succeeded yet",
        )));
    }
    Ok(())
}

pub(super) fn pending_error(error: OpError) -> OpError {
    let mut pending = OpError::new(
        "actor_switch_pending",
        format!(
            "This member's last runtime switch has not finished ({}). It receives no messages until it does; retry the switch completion.",
            error.message
        ),
    );
    pending.details.insert("cause".into(), json!(error.code));
    pending
}

/// At daemon start, before IPC and runtime restore: complete every unfinished
/// switch of the Group. A failure keeps that member held, never the daemon.
pub(crate) fn complete_stranded(home: &HomeLayout, group: &GroupDoc) {
    for actor in &group.actors {
        if runtime_switches::pending_for(group, actor).is_none() {
            continue;
        }
        if let Err(error) = complete_pending(home, &group.group_id, &actor.id) {
            cccc_runtime::diagnostic(format_args!(
                "Runtime switch completion is still pending: group_id={} actor_id={} code={}; the member stays held until a retry succeeds",
                group.group_id, actor.id, error.code
            ));
        }
    }
}

/// Event ids are fixed by the switch, so a repeated completion finds what an
/// interrupted one already wrote instead of writing it twice.
fn derived_id(op_id: &str, part: &str) -> String {
    Sha256::digest(format!("cccc.runtime_switch:{op_id}:{part}"))[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Newest first: the most recent handoffs are the likeliest to need action.
fn describe(ledger_path: &Path, ids: &[String]) -> Result<Vec<Value>, OpError> {
    ids.iter()
        .rev()
        .map(|id| {
            let event = ledger::find_event(ledger_path, id).map_err(OpError::io)?;
            Ok(json!({
                "event_id": id,
                "by": event.as_ref().map(|event| event.by.clone()).unwrap_or_default(),
                "ts": event.as_ref().map(|event| event.ts.clone()).unwrap_or_default(),
            }))
        })
        .collect()
}

fn notice(
    group_id: &str,
    actor: &Actor,
    record: &SwitchRecord,
    notice_id: &str,
    unconfirmed: &[Value],
) -> Event {
    let mut lines = vec![
        format!(
            "You are now {}. The user switched this member from {} to {}.",
            actor.id,
            endpoint(&record.from),
            endpoint(&record.to)
        ),
        "Carried over: this member's tasks, agent state, Mail, reply requests and message history."
            .to_owned(),
        "CCCC does not move the previous runtime's own conversation or anything it did not record in CCCC. Whether that conversation is still visible or can be resumed depends on the provider and its configuration."
            .to_owned(),
        "First call cccc_bootstrap. The previous runtime may not have handled every message it received: check the message history, your tasks and the working directory."
            .to_owned(),
    ];
    if !unconfirmed.is_empty() {
        lines.push(format!(
            "{} message(s) may have reached the previous runtime; their receipt was not confirmed. They will not be delivered again. Read each with cccc_message_history(event_id) and decide whether it still needs handling:",
            unconfirmed.len()
        ));
        for item in unconfirmed.iter().take(LISTED) {
            let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
            lines.push(format!(
                "- {} from {} at {}",
                text("event_id"),
                text("by"),
                text("ts")
            ));
        }
        if unconfirmed.len() > LISTED {
            lines.push(format!(
                "...and {} older ones. The full list is in the `unconfirmed` field of this notice: cccc_message_history(event_id=\"{notice_id}\").",
                unconfirmed.len() - LISTED
            ));
        }
    }
    lines.push(
        "Finally, send the user one short message: what you took over and what you will do next."
            .to_owned(),
    );
    let mut event = Event::new("system.notify", group_id);
    event.by = "system".into();
    event.data = json!({
        "kind": "runtime_switch_handoff",
        "target_actor_id": actor.id,
        "title": "Runtime switched",
        "message": lines.join("\n"),
        "op_id": record.op_id,
        "from": record.from,
        "to": record.to,
        "unconfirmed": unconfirmed,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    event
}

fn endpoint(endpoint: &Endpoint) -> String {
    if endpoint.is_native() {
        format!(
            "the native {} session {}",
            endpoint.runtime, endpoint.source_id
        )
    } else {
        format!("a CCCC-managed {} runtime", endpoint.runtime)
    }
}

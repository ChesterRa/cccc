//! Writing Mail never starts an Actor or resumes a Group (CCCS 6.1), so a
//! sender can hand work to a recipient that will not see it for a while. After
//! a local Mail is written, its result may list the explicitly addressed
//! recipients that are disabled, reported as not running, or in a paused or
//! stopped Group, as the daemon reports them at that moment. These are states,
//! not proof that a recipient cannot read the Mail. Nothing is started,
//! delivered or changed.
//!
//! A Send from an agent enables no disabled Actor and resumes no Group, so its
//! result names those recipients too. Delivery does start an enabled Actor.

use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupDoc, HomeLayout, actors, inbox};
use serde_json::{Value, json};

pub(super) const NOTE: &str = "Recipient states as the daemon reported them when this Mail was written. Writing Mail does not start an Actor or resume a Group. Send, or cccc_message_deliver for this Mail, can request delivery to an enabled Actor in an active Group.";

pub(super) const SEND_NOTE: &str = "Recipient states as the daemon reported them when this message was sent. A message from an agent does not enable a disabled Actor or resume a paused or stopped Group, so nothing is being delivered to these recipients for now.";

/// One entry per named recipient in one of those states. Mail to a
/// broadcast selector gets none, and an empty list says nothing about whether
/// the other recipients will read it.
pub(super) fn advisories(home: &HomeLayout, group: &GroupDoc, mail: &Event) -> Vec<Value> {
    report(group, mail, |actor, reasons| {
        // Only a process CCCC manages, DeepSeek's included, has a running state
        // worth reporting here. A native session belongs to its own app, and a Web
        // Model's enabled flag does not say whether a browser is connected.
        if reasons.is_empty()
            && actor.external_session_id.is_empty()
            && !actor.runtime.is_web_model()
            && !super::actor_runtime_status::resolve(home, group, actor).running
        {
            reasons.push("not_running");
        }
    })
}

/// The same for a Send from an agent. An enabled Actor that is not running is
/// left out, because delivery starts it.
pub(super) fn send_advisories(group: &GroupDoc, message: &Event) -> Vec<Value> {
    report(group, message, |_, _| {})
}

fn report(
    group: &GroupDoc,
    event: &Event,
    mut extend: impl FnMut(&Actor, &mut Vec<&'static str>),
) -> Vec<Value> {
    let mut targets = event
        .data
        .get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .peekable();
    if targets.peek().is_none()
        || targets.any(|target| matches!(target, "@all" | "@peers" | "@foreman"))
    {
        return Vec::new();
    }
    let group_reason = match group.state {
        GroupState::Paused => Some("group_paused"),
        GroupState::Stopped => Some("group_stopped"),
        _ => None,
    };
    actors::visible(group)
        .filter(|actor| actor.id != event.by && inbox::is_for_actor(group, event, &actor.id))
        .filter_map(|actor| {
            let mut reasons = Vec::new();
            if !actor.enabled {
                reasons.push("disabled");
            }
            reasons.extend(group_reason);
            extend(actor, &mut reasons);
            (!reasons.is_empty()).then(|| json!({"actor_id": actor.id, "reasons": reasons}))
        })
        .collect()
}

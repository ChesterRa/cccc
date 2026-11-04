//! Delivery headers name a parent by its short `(reply:<id>)` hint, and agents
//! quote events the same way. Where an agent passes an event ID back, a short
//! ID that matches no event resolves to the one event that agent can read
//! whose ID starts with it. A full ID behaves exactly as before.

use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout, ledger};

use crate::dispatch::{OpError, string_arg};

/// The delivery hint prints eight characters; event IDs have 32.
const MIN_PREFIX_LEN: usize = 8;
const FULL_ID_LEN: usize = 32;

/// Whether `raw` has the shape of a shortened event ID: 8 to 31 lowercase
/// hexadecimal characters. Anything else is only ever matched exactly.
pub(super) fn is_prefix(raw: &str) -> bool {
    (MIN_PREFIX_LEN..FULL_ID_LEN).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// An actor can read the messages it sent and the events delivered to it.
/// This is the visibility of an exact read.
pub(super) fn readable_by(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    (event.kind == "chat.message" && event.by == actor_id)
        || super::actor_delivery::event_targets_actor(group, event, actor_id)
}

/// The one event in `events[from..]` that `readable` accepts and whose ID
/// starts with `prefix`. Events the caller cannot read never count, so an
/// ambiguity error names only events the caller can already see.
pub(super) fn resolve<'a>(
    events: &'a [Event],
    from: usize,
    prefix: &str,
    readable: impl Fn(&Event) -> bool,
) -> Result<Option<&'a Event>, OpError> {
    let mut matches = events
        .get(from..)
        .unwrap_or_default()
        .iter()
        .filter(|event| event.id.starts_with(prefix) && readable(event));
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    let others: Vec<&str> = matches.map(|event| event.id.as_str()).collect();
    if others.is_empty() {
        return Ok(Some(first));
    }
    let mut ids = vec![first.id.as_str()];
    ids.extend(others);
    Err(OpError::new(
        "event_id_ambiguous",
        format!(
            "{prefix} starts several event IDs you can read: {}; pass the full event_id",
            ids.join(", ")
        ),
    ))
}

/// The full ID behind a reply's shortened `reply_to`, resolved among the
/// events its author can read in that author's current generation. Returns
/// `None` when nothing needs resolving: a full or exactly matching ID, or an
/// author other than one of the Group's agents. The user's ports send full IDs.
pub(super) fn full_reply_target(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<Option<String>, OpError> {
    let Some(reply_to) = string_arg(request, "reply_to").filter(|raw| is_prefix(raw)) else {
        return Ok(None);
    };
    let group = super::messaging::load(home, request)?;
    let Some(author) =
        string_arg(request, "by").filter(|by| cccc_core::actors::find(&group, by).is_some())
    else {
        return Ok(None);
    };
    let path = crate::dispatch::store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    ledger::inspect_status(&path, |events, positions, generations| {
        if positions.contains_key(&reply_to) {
            return Ok(None);
        }
        let generation = generations.get(&author).copied().unwrap_or(0);
        let event = resolve(events, generation, &reply_to, |event| {
            readable_by(&group, event, &author)
        })?
        .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {reply_to}")))?;
        Ok(Some(event.id.clone()))
    })
    .map_err(OpError::io)?
}

/// `request` with its `reply_to` replaced by the resolved full ID, so every
/// later step (idempotency, routing and both ledgers) sees only full IDs.
pub(super) fn with_full_reply_target(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<Option<DaemonRequest>, OpError> {
    Ok(full_reply_target(home, request)?.map(|full| {
        let mut resolved = request.clone();
        resolved
            .args
            .insert("reply_to".into(), serde_json::Value::String(full));
        resolved
    }))
}

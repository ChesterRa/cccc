use std::collections::{BTreeMap, HashMap, HashSet};

use cccc_contracts::{Actor, Event};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::notice_event;
use crate::{GroupDoc, actors};

pub(crate) const KIND: &str = "reply_sender_notice";

pub fn is_notice(event: &Event) -> bool {
    event.kind == "system.notify"
        && event.by == "system"
        && event.data.get("kind").and_then(Value::as_str) == Some(KIND)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Obligation {
    pub recipient_actor_id: String,
    pub recipient_actor_created_at: String,
    pub source_event_id: String,
    pub accepted_at: String,
}

impl Obligation {
    pub(crate) fn new(recipient: &Actor, source: &str, accepted_at: &str) -> Self {
        Self {
            recipient_actor_id: recipient.id.clone(),
            recipient_actor_created_at: recipient.created_at.clone(),
            source_event_id: source.to_owned(),
            accepted_at: accepted_at.to_owned(),
        }
    }

    pub(crate) fn into_claim(self) -> (String, String, String) {
        (
            self.recipient_actor_id,
            self.recipient_actor_created_at,
            self.source_event_id,
        )
    }
}

pub fn obligations(event: &Event) -> Vec<Obligation> {
    if !is_notice(event) {
        return Vec::new();
    }
    event
        .data
        .get("context")
        .and_then(|context| context.get("obligations"))
        .and_then(|items| serde_json::from_value(items.clone()).ok())
        .unwrap_or_default()
}

/// Only the original local sender can be reminded. Relayed and imported
/// messages do not establish the current Group's sender identity.
pub(crate) fn local_sender(event: &Event) -> Option<&str> {
    if matches!(event.by.as_str(), "" | "user" | "system")
        || event.data.contains_key("connect_message")
        || event
            .data
            .get("src_group_id")
            .and_then(Value::as_str)
            .is_some_and(|group| !group.trim().is_empty())
    {
        return None;
    }
    Some(&event.by)
}

#[derive(Default)]
pub(crate) struct SenderRegistrations {
    latest: HashMap<String, (usize, String)>,
    last_import: Option<usize>,
}

impl SenderRegistrations {
    pub(crate) fn note(&mut self, event: &Event, position: usize) {
        if event.kind == "actor.add"
            && let Some(actor) = event.data.get("actor")
            && let Some(actor_id) = actor.get("id").and_then(Value::as_str)
        {
            if let Ok(recorded) = serde_json::from_value::<Actor>(actor.clone()) {
                self.latest.insert(
                    actor_id.to_owned(),
                    (position, actors::generation_identity(&recorded)),
                );
            } else {
                self.latest.remove(actor_id);
            }
        } else if event.kind == "group.create"
            && event.data.get("imported") == Some(&Value::Bool(true))
        {
            self.last_import = Some(position);
        }
    }

    pub(crate) fn eligible<'a>(
        &self,
        sender: Option<&str>,
        source_position: usize,
        eligible: &[&'a Actor],
    ) -> Option<&'a Actor> {
        let sender = sender?;
        let (registered, identity) = self.latest.get(sender)?;
        if *registered > source_position
            || self
                .last_import
                .is_some_and(|import| source_position <= import)
        {
            return None;
        }
        eligible
            .iter()
            .copied()
            .find(|actor| actor.id == sender && actors::generation_identity(actor) == *identity)
    }
}

// Both reducers retain the recipient/source order within each sender and
// write sender notices first, so their claims survive a partial append.
#[derive(Default)]
pub(crate) struct Batch<'a>(BTreeMap<&'a str, (&'a Actor, Vec<Obligation>)>);

impl<'a> Batch<'a> {
    pub(crate) fn add(&mut self, sender: &'a Actor, obligation: Obligation) {
        self.0
            .entry(&sender.id)
            .or_insert_with(|| (sender, Vec::new()))
            .1
            .push(obligation);
    }

    pub(crate) fn finish(self, group: &GroupDoc, recipient_notices: Vec<Event>) -> Vec<Event> {
        if self.0.is_empty() {
            return recipient_notices;
        }
        let mut notices = Vec::with_capacity(self.0.len() + recipient_notices.len());
        for (sender, obligations) in self.0.into_values() {
            let mut seen = HashSet::new();
            let sources = obligations
                .iter()
                .filter(|item| seen.insert(item.source_event_id.as_str()))
                .map(|item| item.source_event_id.clone())
                .collect();
            let mut event = notice_event(
                group,
                sender,
                KIND,
                "Handoff replies pending",
                "Accepted handoffs still awaited replies when this notice was sent. Check the original messages.",
                sources,
            );
            let context = event.data.get_mut("context").expect("notice context");
            context["count"] = json!(obligations.len());
            context["obligations"] = json!(obligations);
            notices.push(event);
        }
        notices.extend(recipient_notices);
        notices
    }
}

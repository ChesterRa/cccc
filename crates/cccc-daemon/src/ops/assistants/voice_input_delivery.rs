use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};

use crate::dispatch::OpError;
use crate::ops::{actor_delivery, actor_runtime};

const ACTOR_ID: &str = "voice-secretary";

#[derive(Default)]
pub(super) struct DeliveryOutcome {
    pub(super) event: Option<Event>,
    pub(super) notify: Option<Event>,
    pub(super) delivery: Option<Value>,
    pub(super) actor_woken: bool,
    pub(super) wake_error: String,
}

pub(super) fn deliver(
    home: &HomeLayout,
    store: &GroupStore,
    group_id: &str,
    session_id: &str,
    segment_id: &str,
    by: &str,
    candidate_input: Option<&Value>,
) -> Result<DeliveryOutcome, OpError> {
    let Some(input) = candidate_input else {
        return Ok(DeliveryOutcome::default());
    };
    let group = store.load(group_id).map_err(OpError::not_found)?;
    let needs_notice = group.actors.iter().any(|actor| actor.id == ACTOR_ID);
    let (prior_input, prior_notice) = events_for_segment(store, group_id, session_id, segment_id)?;
    if prior_input.is_some() && (!needs_notice || prior_notice.is_some()) {
        return Ok(DeliveryOutcome::default());
    }

    let ledger_path = store.ledger_path(group_id).map_err(OpError::io)?;
    let input_event = if let Some(event) = prior_input {
        event
    } else {
        let mut event = Event::new("assistant.voice.input", group_id);
        event.by = by.into();
        event.data = input.as_object().cloned().unwrap_or_default();
        ledger::append(&ledger_path, &event).map_err(OpError::io)?;
        event
    };
    let mut outcome = DeliveryOutcome {
        event: Some(input_event),
        ..DeliveryOutcome::default()
    };

    if !needs_notice {
        return Ok(outcome);
    }
    if cccc_runtime::status(group_id, ACTOR_ID).is_ok_and(|status| status.running) {
        outcome.actor_woken = true;
    } else if group.running {
        match actor_runtime::apply(home, &group, ACTOR_ID, "actor.start") {
            Ok(status) => outcome.actor_woken = status.is_some_and(|item| item.running),
            Err(error) => outcome.wake_error = format!("{}: {}", error.code, error.message),
        }
    }
    let notice = if let Some(event) = prior_notice {
        event
    } else {
        let mut event = Event::new("system.notify", group_id);
        event.by = "system".into();
        event.data = json!({
            "kind":"voice_secretary_input",
            "title":"Voice Secretary input",
            "text":"New voice input is ready.",
            "to":[ACTOR_ID],
            "priority":"normal",
            "context":{"kind":"voice_secretary_input","input_envelope":input}
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(&ledger_path, &event).map_err(OpError::io)?;
        event
    };
    outcome.delivery = serde_json::to_value(actor_delivery::dispatch(home, &group, &notice)).ok();
    outcome.notify = Some(notice);
    Ok(outcome)
}

fn events_for_segment(
    store: &GroupStore,
    group_id: &str,
    session_id: &str,
    segment_id: &str,
) -> Result<(Option<Event>, Option<Event>), OpError> {
    let events = ledger::read_all(&store.ledger_path(group_id).map_err(OpError::io)?)
        .map_err(OpError::io)?;
    let input = events
        .iter()
        .find(|event| {
            event.kind == "assistant.voice.input"
                && event_data_string(event, &["session_id"]) == Some(session_id)
                && event_data_string(event, &["segment_id"]) == Some(segment_id)
        })
        .cloned();
    let notice = events
        .iter()
        .find(|event| {
            event.kind == "system.notify"
                && event_data_string(event, &["kind"]) == Some("voice_secretary_input")
                && event_data_string(event, &["context", "input_envelope", "session_id"])
                    == Some(session_id)
                && event_data_string(event, &["context", "input_envelope", "segment_id"])
                    == Some(segment_id)
        })
        .cloned();
    Ok((input, notice))
}

fn event_data_string<'a>(event: &'a Event, path: &[&str]) -> Option<&'a str> {
    let (first, rest) = path.split_first()?;
    let mut value = event.data.get(*first)?;
    for key in rest {
        value = value.get(*key)?;
    }
    value.as_str()
}

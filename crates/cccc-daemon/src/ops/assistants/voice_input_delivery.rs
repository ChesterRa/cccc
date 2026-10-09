use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::Value;

use crate::dispatch::OpError;

#[derive(Default)]
pub(super) struct DeliveryOutcome {
    pub(super) event: Option<Event>,
    pub(super) wake_error: String,
    pub(super) task_id: Option<String>,
    pub(super) processing_deferred: bool,
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
    let prior_input = events_for_segment(store, group_id, session_id, segment_id)?;
    let ledger_path = store.ledger_path(group_id).map_err(OpError::io)?;
    let input_event = if let Some(event) = prior_input {
        event
    } else {
        let mut event = Event::new("assistant.voice.input", group_id);
        event.scope_key = input["scope_key"].as_str().unwrap_or("").into();
        event.by = by.into();
        event.data = input.as_object().cloned().unwrap_or_default();
        ledger::append(&ledger_path, &event).map_err(OpError::io)?;
        event
    };
    let mut outcome = DeliveryOutcome {
        event: Some(input_event),
        ..DeliveryOutcome::default()
    };

    match crate::ops::voice_secretary::enqueue(home, group_id, input) {
        Ok(id) => outcome.task_id = id,
        Err(error) => outcome.wake_error = format!("Processing deferred; source saved: {error}"),
    }
    outcome.processing_deferred = outcome.task_id.is_none();
    Ok(outcome)
}

fn events_for_segment(
    store: &GroupStore,
    group_id: &str,
    session_id: &str,
    segment_id: &str,
) -> Result<Option<Event>, OpError> {
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
    Ok(input)
}

fn event_data_string<'a>(event: &'a Event, path: &[&str]) -> Option<&'a str> {
    let (first, rest) = path.split_first()?;
    let mut value = event.data.get(*first)?;
    for key in rest {
        value = value.get(*key)?;
    }
    value.as_str()
}

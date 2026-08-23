use cccc_contracts::Event;
use serde_json::Value;
use std::collections::HashMap;

/// Legacy `chat.read.event_id` is an inclusive ledger watermark, not a
/// per-event receipt. Keep that compatibility rule isolated from the current
/// `runtime.delivery` state machine.
pub(super) struct LegacyReadWatermark<'a> {
    event_positions: HashMap<&'a str, usize>,
    inclusive_index: Option<usize>,
}

impl<'a> LegacyReadWatermark<'a> {
    pub(super) fn from_events(events: &'a [Event], actor_id: &str) -> Self {
        let mut event_positions = HashMap::with_capacity(events.len());
        for (index, event) in events.iter().enumerate() {
            event_positions.entry(event.id.as_str()).or_insert(index);
        }

        let inclusive_index = events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.kind == "chat.read"
                    && event.data.get("actor_id").and_then(Value::as_str) == Some(actor_id)
            })
            .filter_map(|(read_index, event)| {
                let target = event.data.get("event_id").and_then(Value::as_str)?;
                event_positions
                    .get(target)
                    .copied()
                    .filter(|target_index| *target_index <= read_index)
            })
            .max();

        Self {
            event_positions,
            inclusive_index,
        }
    }

    pub(super) fn covers_notification(&self, event: &Event) -> bool {
        if event.kind != "system.notify" {
            return false;
        }
        let Some(inclusive_index) = self.inclusive_index else {
            return false;
        };
        if self
            .event_positions
            .get(event.id.as_str())
            .is_some_and(|index| *index <= inclusive_index)
        {
            return true;
        }

        [
            event.data.get("event_id"),
            event.data.get("related_event_id"),
            event
                .data
                .get("context")
                .and_then(Value::as_object)
                .and_then(|context| context.get("event_id")),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|event_id| {
            self.event_positions
                .get(event_id)
                .is_some_and(|index| *index <= inclusive_index)
        })
    }
}

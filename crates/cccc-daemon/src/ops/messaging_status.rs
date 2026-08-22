use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout, actors, inbox, ledger};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::dispatch::{OpError, OpResult, object, required_arg, store};

const MAX_STATUS_EVENT_IDS: usize = 1000;

type DeliveryStatuses = HashMap<String, HashMap<String, String>>;
type ReplyPositions = HashMap<String, HashMap<String, usize>>;
type CancellationPositions = HashMap<String, usize>;

pub fn statuses(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_ids = normalized_event_ids(request);
    if event_ids.is_empty() {
        return object(json!({"statuses": {}}));
    }
    let statuses =
        StatusSnapshot::with(home, &group_id, |snapshot| snapshot.for_events(&event_ids))?;
    object(json!({"statuses": statuses}))
}

pub(super) fn for_events(
    home: &HomeLayout,
    group_id: &str,
    event_ids: &[String],
) -> Result<BTreeMap<String, Value>, OpError> {
    StatusSnapshot::with(home, group_id, |snapshot| snapshot.for_events(event_ids))
}

pub fn read_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_id = required_arg(request, "event_id")?;
    let read_status = StatusSnapshot::with(home, &group_id, |snapshot| {
        snapshot
            .for_events(std::slice::from_ref(&event_id))
            .remove(&event_id)
            .and_then(|value| value.get("read_status").cloned())
            .unwrap_or_else(|| json!({}))
    })?;
    object(json!({"event_id": event_id, "read_status": read_status}))
}

struct StatusSnapshot<'a> {
    group: GroupDoc,
    events: &'a [Event],
    positions: &'a HashMap<String, usize>,
    cursor_positions: HashMap<String, usize>,
    actor_generations: HashMap<String, usize>,
    reply_positions: ReplyPositions,
    delivery_statuses: DeliveryStatuses,
    cancellation_positions: CancellationPositions,
}

impl StatusSnapshot<'_> {
    fn with<T>(
        home: &HomeLayout,
        group_id: &str,
        use_snapshot: impl FnOnce(&StatusSnapshot<'_>) -> T,
    ) -> Result<T, OpError> {
        let group = store(home)?
            .load(group_id)
            .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))?;
        let path = store(home)?.ledger_path(group_id).map_err(OpError::io)?;
        let cursors = inbox::cursors(home, group_id).map_err(OpError::io)?;
        ledger::inspect_status(&path, |events, positions, _replied_by| {
            let cursor_positions = cursors
                .into_iter()
                .filter_map(|(actor_id, event_id)| {
                    positions
                        .get(&event_id)
                        .copied()
                        .map(|index| (actor_id, index))
                })
                .collect();
            let actor_generations = inbox::actor_generation_positions(events);
            let (delivery_statuses, reply_positions, cancellation_positions) =
                collect_message_outcomes(events);
            use_snapshot(&StatusSnapshot {
                group,
                events,
                positions,
                cursor_positions,
                actor_generations,
                reply_positions,
                delivery_statuses,
                cancellation_positions,
            })
        })
        .map_err(OpError::io)
    }

    fn for_events(&self, event_ids: &[String]) -> BTreeMap<String, Value> {
        let requested = event_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        self.events
            .iter()
            .filter(|event| event.kind == "chat.message" && requested.contains(event.id.as_str()))
            .map(|event| (event.id.clone(), self.status(event)))
            .collect()
    }

    fn status(&self, event: &Event) -> Value {
        let recipients = self.actor_recipients(event);
        let mut status = Map::new();
        if event.data.get("message_mode").and_then(Value::as_str) == Some("mail") {
            let read_status = recipients
                .iter()
                .map(|actor_id| (actor_id.clone(), Value::Bool(self.is_read(event, actor_id))))
                .collect::<Map<_, _>>();
            status.insert("read_status".into(), Value::Object(read_status));
        }
        if is_cross_group_source(event) {
            return Value::Object(status);
        }

        let obligation_recipients = self.obligation_recipients(event, recipients);
        let reply_requested =
            event.data.get("message_mode").and_then(Value::as_str) == Some("request_reply");
        let replies = self.reply_positions.get(&event.id);
        let cancellation = self.cancellation_positions.get(&event.id).copied();
        let delivery = self.delivery_statuses.get(&event.id);

        let obligation_status = obligation_recipients
            .into_iter()
            .map(|actor_id| {
                let (replied, cancelled) = terminal_outcome(
                    replies.and_then(|actors| actors.get(&actor_id)).copied(),
                    cancellation,
                );
                (
                    actor_id.clone(),
                    json!({
                        "replied": replied,
                        "reply_requested": reply_requested,
                        "cancelled": reply_requested && cancelled,
                        "delivery_state":delivery
                            .and_then(|states| states.get(&actor_id))
                            .map(String::as_str)
                            .unwrap_or(""),
                    }),
                )
            })
            .collect::<Map<_, _>>();
        status.insert("obligation_status".into(), Value::Object(obligation_status));
        Value::Object(status)
    }

    fn actor_recipients(&self, event: &Event) -> Vec<String> {
        actors::visible(&self.group)
            .filter(|actor| actor.id != event.by)
            .filter(|actor| {
                inbox::actor_generation_contains(
                    &self.actor_generations,
                    self.positions,
                    &actor.id,
                    event,
                )
                .unwrap_or_else(|| actor.created_at.is_empty() || actor.created_at <= event.ts)
            })
            .filter(|actor| inbox::is_for_actor(&self.group, event, &actor.id))
            .map(|actor| actor.id.clone())
            .collect()
    }

    fn obligation_recipients(&self, event: &Event, mut recipients: Vec<String>) -> Vec<String> {
        let explicitly_targets_user =
            event
                .data
                .get("to")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|recipient| matches!(recipient, "user" | "@user"))
                });
        if event.by != "user" && explicitly_targets_user {
            recipients.push("user".into());
        }
        recipients
    }

    fn is_read(&self, event: &Event, actor_id: &str) -> bool {
        let Some(event_position) = self.positions.get(&event.id) else {
            return false;
        };
        self.cursor_positions
            .get(actor_id)
            .is_some_and(|cursor| cursor >= event_position)
    }
}

fn collect_message_outcomes(
    events: &[Event],
) -> (DeliveryStatuses, ReplyPositions, CancellationPositions) {
    let mut deliveries = HashMap::<String, HashMap<String, String>>::new();
    let mut replies = HashMap::<String, HashMap<String, usize>>::new();
    let mut cancellations = HashMap::<String, usize>::new();
    for (position, event) in events.iter().enumerate() {
        if event.kind == "chat.message" {
            if let Some(source_event_id) = event
                .data
                .get("reply_to")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                replies
                    .entry(source_event_id.to_owned())
                    .or_default()
                    .entry(event.by.clone())
                    .or_insert(position);
            }
            continue;
        }
        if event.kind == "chat.reply_request.cancelled" {
            if let Some(source_event_id) = event
                .data
                .get("source_event_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                cancellations
                    .entry(source_event_id.to_owned())
                    .or_insert(position);
            }
            continue;
        }
        if event.kind != "runtime.delivery" {
            continue;
        }
        if let (Some(source_event_id), Some(actor_id), Some(state)) = (
            event.data.get("source_event_id").and_then(Value::as_str),
            event.data.get("actor_id").and_then(Value::as_str),
            event.data.get("state").and_then(Value::as_str),
        ) {
            deliveries
                .entry(source_event_id.to_owned())
                .or_default()
                .insert(actor_id.to_owned(), state.to_owned());
        }
    }
    (deliveries, replies, cancellations)
}

fn terminal_outcome(
    reply_position: Option<usize>,
    cancellation_position: Option<usize>,
) -> (bool, bool) {
    let cancelled = cancellation_position
        .is_some_and(|cancelled| reply_position.is_none_or(|replied| cancelled < replied));
    (reply_position.is_some() && !cancelled, cancelled)
}

fn normalized_event_ids(request: &DaemonRequest) -> Vec<String> {
    let mut seen = HashSet::new();
    request
        .args
        .get("event_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|event_id| !event_id.is_empty())
        .filter(|event_id| seen.insert((*event_id).to_owned()))
        .take(MAX_STATUS_EVENT_IDS)
        .map(str::to_owned)
        .collect()
}

fn is_cross_group_source(event: &Event) -> bool {
    event
        .data
        .get("dst_group_id")
        .and_then(Value::as_str)
        .is_some_and(|group_id| !group_id.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_runtime_delivery_state_wins_per_recipient() {
        let mut claimed = Event::new("runtime.delivery", "g_one");
        claimed.data = json!({"source_event_id":"event-1","actor_id":"peer1","state":"claimed"})
            .as_object()
            .cloned()
            .expect("data");
        let mut accepted = Event::new("runtime.delivery", "g_one");
        accepted.data = json!({"source_event_id":"event-1","actor_id":"peer1","state":"accepted"})
            .as_object()
            .cloned()
            .expect("data");

        let (statuses, _, _) = collect_message_outcomes(&[claimed, accepted]);
        assert_eq!(statuses["event-1"]["peer1"], "accepted");
    }

    #[test]
    fn first_reply_or_cancellation_is_terminal() {
        assert_eq!(terminal_outcome(Some(1), Some(2)), (true, false));
        assert_eq!(terminal_outcome(Some(2), Some(1)), (false, true));
        assert_eq!(terminal_outcome(Some(1), None), (true, false));
        assert_eq!(terminal_outcome(None, Some(1)), (false, true));
    }
}

use cccc_contracts::reply_question::{self, Stored};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout, actors, inbox, ledger, user_requests};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::dispatch::{OpError, OpResult, object, required_arg, store};

const MAX_STATUS_EVENT_IDS: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Requester {
    Available,
    /// Removed, or registered again after asking.
    Unavailable,
    /// The ledger cannot prove the registration either way.
    Unknown,
    /// Relayed from another Group or instance, or not an Actor here; answer
    /// through an ordinary reply.
    Unsupported,
}

impl Requester {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        }
    }
}

/// The user's side of one request, read under the caller's Group permit.
pub(super) struct UserRequest {
    pub target: Event,
    /// `None` while open; otherwise `answered` or `cancelled`.
    pub closed: Option<&'static str>,
    pub requester: Requester,
}

/// `Ok(None)` when the event is not a request to the user.
pub(super) fn user_request(
    home: &HomeLayout,
    group_id: &str,
    event_id: &str,
) -> Result<Option<UserRequest>, OpError> {
    let event_ids = [event_id.to_owned()];
    StatusSnapshot::with(home, group_id, &event_ids, |snapshot| {
        let target = snapshot
            .positions
            .get(event_id)
            .and_then(|&position| snapshot.events.get(position))
            .cloned()
            .ok_or_else(|| {
                OpError::new("event_not_found", format!("event not found: {event_id}"))
            })?;
        let requested =
            target.data.get("message_mode").and_then(Value::as_str) == Some("request_reply");
        if !requested || !user_requests::targets_user(&target) {
            return Ok(None);
        }
        let user_reply = snapshot
            .reply_positions
            .get(event_id)
            .and_then(|actors| actors.get("user"))
            .copied();
        let cancellation = snapshot.cancellation_positions.get(event_id).copied();
        let closed = match terminal_outcome(user_reply, cancellation) {
            (true, _) => Some("answered"),
            (_, true) => Some("cancelled"),
            _ => None,
        };
        Ok(Some(UserRequest {
            requester: snapshot.requester(&target),
            target,
            closed,
        }))
    })?
}

type DeliveryStatuses = HashMap<String, HashMap<String, String>>;
type ReplyPositions = HashMap<String, HashMap<String, usize>>;
type CancellationPositions = HashMap<String, usize>;

pub fn statuses(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_ids = normalized_event_ids(request);
    if event_ids.is_empty() {
        return object(json!({"statuses": {}}));
    }
    let statuses = StatusSnapshot::with(home, &group_id, &event_ids, |snapshot| {
        snapshot.for_events(&event_ids)
    })?;
    object(json!({"statuses": statuses}))
}

pub(super) fn for_events(
    home: &HomeLayout,
    group_id: &str,
    event_ids: &[String],
) -> Result<BTreeMap<String, Value>, OpError> {
    StatusSnapshot::with(home, group_id, event_ids, |snapshot| {
        snapshot.for_events(event_ids)
    })
}

pub fn read_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_id = required_arg(request, "event_id")?;
    let event_ids = std::slice::from_ref(&event_id);
    let read_status = StatusSnapshot::with(home, &group_id, event_ids, |snapshot| {
        snapshot
            .for_events(event_ids)
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
    actor_generations: &'a HashMap<String, usize>,
    reply_positions: ReplyPositions,
    delivery_statuses: DeliveryStatuses,
    cancellation_positions: CancellationPositions,
    connect_receipts: HashMap<String, Value>,
    retired_bridge: HashSet<String>,
}

impl StatusSnapshot<'_> {
    fn with<T>(
        home: &HomeLayout,
        group_id: &str,
        event_ids: &[String],
        use_snapshot: impl FnOnce(&StatusSnapshot<'_>) -> T,
    ) -> Result<T, OpError> {
        let group = store(home)?
            .load(group_id)
            .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))?;
        let path = store(home)?.ledger_path(group_id).map_err(OpError::io)?;
        let cursors = inbox::cursors(home, group_id).map_err(OpError::io)?;
        ledger::inspect_status(&path, |events, positions, actor_generations| {
            let cursor_positions = cursors
                .into_iter()
                .filter_map(|(actor_id, event_id)| {
                    positions
                        .get(&event_id)
                        .copied()
                        .map(|index| (actor_id, index))
                })
                .collect();
            let scan_start = event_ids
                .iter()
                .filter_map(|event_id| positions.get(event_id.as_str()).copied())
                .filter(|&pos| events.get(pos).is_some_and(|e| e.kind == "chat.message"))
                .min()
                .unwrap_or(events.len());
            let tail_events = events.get(scan_start..).unwrap_or_default();
            let (delivery_statuses, reply_positions, cancellation_positions) =
                collect_message_outcomes_from(tail_events, scan_start);
            let connect_receipts = tail_events.iter().filter(|event| event.kind == "chat.cross_group_receipt" && event.by == "system" && event.data.get("transport").and_then(Value::as_str) == Some("connect")).filter_map(|event| {
                let source = event.data.get("source_event_id")?.as_str()?;
                Some((source.to_owned(), json!({"state":event.data.get("status"), "error":event.data.get("error"), "remote_event_id":event.data.get("remote_event_id")})))
            }).collect();
            let retired_bridge = tail_events.iter().filter(|event| cccc_core::group_bridge_retirement::is_retired_receipt(event)).filter_map(|event| event.data.get("source_event_id")?.as_str().map(str::to_owned)).collect();
            use_snapshot(&StatusSnapshot {
                group,
                events,
                positions,
                cursor_positions,
                actor_generations,
                reply_positions,
                delivery_statuses,
                cancellation_positions,
                connect_receipts,
                retired_bridge,
            })
        })
        .map_err(OpError::io)
    }

    fn for_events(&self, event_ids: &[String]) -> BTreeMap<String, Value> {
        event_ids
            .iter()
            .filter_map(|event_id| {
                let &position = self.positions.get(event_id.as_str())?;
                let event = self.events.get(position)?;
                (event.kind == "chat.message").then(|| (event.id.clone(), self.status(event)))
            })
            .collect()
    }

    fn status(&self, event: &Event) -> Value {
        let recipients = self.actor_recipients(event);
        let mut status = Map::new();
        if self.retired_bridge.contains(&event.id)
            || cccc_core::group_bridge_retirement::is_retired_message(event)
        {
            status.insert("retired_bridge".into(), json!(true));
        }
        if event.data.get("message_mode").and_then(Value::as_str) == Some("mail") {
            let read_status = recipients
                .iter()
                .map(|actor_id| (actor_id.clone(), Value::Bool(self.is_read(event, actor_id))))
                .collect::<Map<_, _>>();
            status.insert("read_status".into(), Value::Object(read_status));
        }
        if event.data.contains_key("connect_message")
            && let Some(position) = self.cancellation_positions.get(&event.id)
            && let Some(cancel) = self.events.get(*position)
            && cancel.data.contains_key("connect_cancel")
        {
            let propagation = if cancel.data.contains_key("connect_cancel_sha256") {
                json!({"state":"sent"})
            } else {
                self.connect_receipts
                    .get(&cancel.id)
                    .cloned()
                    .unwrap_or_else(|| json!({"state":"queued"}))
            };
            status.insert("connect_cancellation".into(), propagation);
        }
        if is_cross_group_source(event) {
            if let Ok(message) = super::connect_messages::stored_message(event) {
                status.insert(
                    "connect_delivery".into(),
                    self.connect_receipts
                        .get(&event.id)
                        .cloned()
                        .unwrap_or_else(|| json!({"state":"queued"})),
                );
                let replies = self.reply_positions.get(&event.id);
                let cancellation = self.cancellation_positions.get(&event.id).copied();
                let reply_requested = message.message_mode == "request_reply";
                let obligations=message.recipients.iter().map(|actor| {
                    let key=connect_reply_author(&message.target.instance_id,actor);
                    let (replied,cancelled)=terminal_outcome(replies.and_then(|actors|actors.get(&key)).copied(),cancellation);
                    (actor.id.clone(),json!({"replied":replied,"reply_requested":reply_requested,"cancelled":reply_requested && cancelled,"delivery_state":""}))
                }).collect::<Map<_,_>>();
                status.insert("obligation_status".into(), Value::Object(obligations));
            }
            return Value::Object(status);
        }

        let obligation_recipients = self.obligation_recipients(event, recipients);
        let reply_requested =
            event.data.get("message_mode").and_then(Value::as_str) == Some("request_reply");
        let replies = self.reply_positions.get(&event.id);
        let cancellation = self.cancellation_positions.get(&event.id).copied();
        let delivery = self.delivery_statuses.get(&event.id);
        if reply_requested && obligation_recipients.iter().any(|id| id == "user") {
            let user_reply = replies.and_then(|actors| actors.get("user")).copied();
            status.insert(
                "user_request".into(),
                self.user_request(event, user_reply, cancellation),
            );
        }

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
                    self.actor_generations,
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
        if user_requests::targets_user(event) {
            recipients.push("user".into());
        }
        recipients
    }

    /// What answering this request to the user in place depends on: whether
    /// its question can be trusted and whether its sender is still the Actor
    /// that asked.
    fn user_request(
        &self,
        event: &Event,
        user_reply: Option<usize>,
        cancellation: Option<usize>,
    ) -> Value {
        let question = match reply_question::stored(&event.data) {
            Stored::None => "none",
            Stored::Valid(_) => "valid",
            Stored::Unavailable => "unavailable",
        };
        let requester = self.requester(event);
        let mut status = Map::new();
        status.insert("question".into(), json!(question));
        status.insert("requester".into(), json!(requester.as_str()));
        if requester == Requester::Unsupported {
            status.insert("reply_route".into(), json!("composer"));
        }
        if terminal_outcome(user_reply, cancellation).0
            && let Some(reply) = user_reply.and_then(|position| self.events.get(position))
        {
            status.insert("reply_event_id".into(), json!(reply.id));
        }
        Value::Object(status)
    }

    /// The sender is still the Actor that asked only when the ledger proves
    /// it: its latest registration precedes the request and records the
    /// identity the Group holds now. Any missing proof is unknown, never
    /// assumed (a creation time can be supplied by a caller, and an add can
    /// stop between saving the Group and appending its event).
    fn requester(&self, event: &Event) -> Requester {
        let relayed = event.data.contains_key("connect_message")
            || event
                .data
                .get("src_group_id")
                .and_then(Value::as_str)
                .is_some_and(|group| !group.trim().is_empty());
        if relayed {
            return Requester::Unsupported;
        }
        let registered = self.actor_generations.get(&event.by).copied();
        let Some(actor) = self.group.actors.iter().find(|actor| actor.id == event.by) else {
            // Without a relay field, a sender the ledger never registered is
            // unproven rather than from elsewhere.
            return if registered.is_some() {
                Requester::Unavailable
            } else {
                Requester::Unknown
            };
        };
        let (Some(registered), Some(&asked)) = (registered, self.positions.get(&event.id)) else {
            return Requester::Unknown;
        };
        if registered > asked {
            return Requester::Unavailable;
        }
        let recorded = self
            .events
            .get(registered)
            .and_then(|add| add.data.get("actor"))
            .and_then(|actor| serde_json::from_value::<cccc_contracts::Actor>(actor.clone()).ok());
        match recorded {
            Some(recorded)
                if recorded.id == actor.id
                    && actors::generation_identity(&recorded)
                        == actors::generation_identity(actor) =>
            {
                Requester::Available
            }
            _ => Requester::Unknown,
        }
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

#[cfg(test)]
fn collect_message_outcomes(
    events: &[Event],
) -> (DeliveryStatuses, ReplyPositions, CancellationPositions) {
    collect_message_outcomes_from(events, 0)
}

fn collect_message_outcomes_from(
    events: &[Event],
    start_position: usize,
) -> (DeliveryStatuses, ReplyPositions, CancellationPositions) {
    let mut deliveries = HashMap::<String, HashMap<String, String>>::new();
    let mut replies = HashMap::<String, HashMap<String, usize>>::new();
    let mut cancellations = HashMap::<String, usize>::new();
    for (offset, event) in events.iter().enumerate() {
        let position = start_position + offset;
        if event.kind == "chat.message" {
            if let Some(source_event_id) = event
                .data
                .get("reply_to")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let author = super::connect_messages::stored_message(event)
                    .ok()
                    .filter(|message| {
                        event.by == format!("connect:{}", message.source.instance_id)
                            && message.reply_to.is_some()
                    })
                    .map(|message| {
                        connect_reply_author(&message.source.instance_id, &message.sender)
                    })
                    .unwrap_or_else(|| event.by.clone());
                replies
                    .entry(source_event_id.to_owned())
                    .or_default()
                    .entry(author)
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

fn connect_reply_author(instance: &str, actor: &cccc_contracts::connect::ConnectActor) -> String {
    format!("connect:{instance}:{}:{}", actor.id, actor.generation)
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

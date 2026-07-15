use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout, actors, inbox, ledger};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::dispatch::{OpError, OpResult, object, required_arg, store};

const MAX_STATUS_EVENT_IDS: usize = 1000;

pub fn statuses(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_ids = normalized_event_ids(request);
    if event_ids.is_empty() {
        return object(json!({"statuses": {}}));
    }
    let snapshot = StatusSnapshot::load(home, &group_id)?;
    object(json!({"statuses": snapshot.for_events(&event_ids)}))
}

pub fn read_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let event_id = required_arg(request, "event_id")?;
    let snapshot = StatusSnapshot::load(home, &group_id)?;
    let read_status = snapshot
        .for_events(std::slice::from_ref(&event_id))
        .remove(&event_id)
        .and_then(|value| value.get("read_status").cloned())
        .unwrap_or_else(|| json!({}));
    object(json!({"event_id": event_id, "read_status": read_status}))
}

struct StatusSnapshot {
    group: GroupDoc,
    events: Vec<Event>,
    positions: HashMap<String, usize>,
    cursor_positions: HashMap<String, usize>,
    acked_by: HashMap<String, BTreeSet<String>>,
    replied_by: HashMap<String, BTreeSet<String>>,
}

impl StatusSnapshot {
    fn load(home: &HomeLayout, group_id: &str) -> Result<Self, OpError> {
        let group = store(home)?.load(group_id).map_err(OpError::not_found)?;
        let path = store(home)?.ledger_path(group_id).map_err(OpError::io)?;
        let events = ledger::read_all(&path).map_err(OpError::io)?;
        let positions = events
            .iter()
            .enumerate()
            .map(|(index, event)| (event.id.clone(), index))
            .collect::<HashMap<_, _>>();
        let cursor_positions = inbox::cursors(home, group_id)
            .map_err(OpError::io)?
            .into_iter()
            .filter_map(|(actor_id, event_id)| {
                positions
                    .get(&event_id)
                    .copied()
                    .map(|index| (actor_id, index))
            })
            .collect();
        let mut acked_by: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut replied_by: HashMap<String, BTreeSet<String>> = HashMap::new();
        for event in &events {
            if event.kind == "chat.ack" {
                let actor_id = event.data.get("actor_id").and_then(Value::as_str);
                relation(&mut acked_by, event, "event_id", actor_id);
            } else if event.kind == "chat.message" {
                relation(&mut replied_by, event, "reply_to", Some(&event.by));
            }
        }
        Ok(Self {
            group,
            events,
            positions,
            cursor_positions,
            acked_by,
            replied_by,
        })
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
        let read_status = recipients
            .iter()
            .map(|actor_id| (actor_id.clone(), Value::Bool(self.is_read(event, actor_id))))
            .collect::<Map<_, _>>();
        let mut status = Map::new();
        status.insert("read_status".into(), Value::Object(read_status));

        if is_cross_group_source(event) {
            return Value::Object(status);
        }

        let obligation_recipients = self.obligation_recipients(event, recipients);
        let is_attention = event.data.get("priority").and_then(Value::as_str) == Some("attention");
        let reply_required = event
            .data
            .get("reply_required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let acked = self.acked_by.get(&event.id);
        let replied = self.replied_by.get(&event.id);

        if is_attention {
            let ack_status = obligation_recipients
                .iter()
                .map(|actor_id| {
                    let value = replied.is_some_and(|actors| actors.contains(actor_id))
                        || acked.is_some_and(|actors| actors.contains(actor_id))
                        || self.is_read(event, actor_id);
                    (actor_id.clone(), Value::Bool(value))
                })
                .collect::<Map<_, _>>();
            status.insert("ack_status".into(), Value::Object(ack_status));
        }

        let obligation_status = obligation_recipients
            .into_iter()
            .map(|actor_id| {
                let read = self.is_read(event, &actor_id);
                let replied = replied.is_some_and(|actors| actors.contains(&actor_id));
                let acked = replied
                    || acked.is_some_and(|actors| actors.contains(&actor_id))
                    || (is_attention && read);
                (
                    actor_id,
                    json!({
                        "read": read,
                        "acked": acked,
                        "replied": replied,
                        "reply_required": reply_required,
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
            .filter(|actor| actor.created_at.is_empty() || actor.created_at <= event.ts)
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

fn relation(
    target: &mut HashMap<String, BTreeSet<String>>,
    event: &Event,
    key: &str,
    actor_id: Option<&str>,
) {
    let Some(target_id) = event.data.get(key).and_then(Value::as_str) else {
        return;
    };
    let actor_id = actor_id.unwrap_or_default();
    if !target_id.is_empty() && !actor_id.is_empty() {
        target
            .entry(target_id.to_owned())
            .or_default()
            .insert(actor_id.to_owned());
    }
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

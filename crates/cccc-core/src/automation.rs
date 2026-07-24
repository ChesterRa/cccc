use cccc_contracts::{Event, GroupState, utc_now};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::io;

use crate::actors;
use crate::automation_render::notify_event;
use crate::automation_schedule::is_due;
use crate::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};

mod state;
use state::RuntimeState;

#[derive(Debug, Clone)]
pub enum ScheduledAction {
    GroupState {
        group_id: String,
        state: String,
    },
    ActorControl {
        group_id: String,
        operation: String,
        targets: Vec<String>,
    },
}

#[derive(Debug, Default)]
pub struct TickResult {
    pub notifications: Vec<Event>,
    pub actions: Vec<ScheduledAction>,
}

pub fn tick(home: &HomeLayout) -> io::Result<TickResult> {
    tick_scheduled(home, true)
}

pub fn tick_scheduled(home: &HomeLayout, include_unread: bool) -> io::Result<TickResult> {
    let mut result = TickResult::default();
    for group_id in group_ids(home)? {
        let group_result = match tick_group(home, &group_id, include_unread) {
            Ok(result) => result,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        result.notifications.extend(group_result.notifications);
        result.actions.extend(group_result.actions);
    }
    Ok(result)
}

pub fn group_ids(home: &HomeLayout) -> io::Result<Vec<String>> {
    let store = GroupStore::new(home.clone())?;
    Ok(store
        .list()?
        .into_iter()
        .map(|group| group.group_id)
        .collect())
}

pub fn tick_group(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
) -> io::Result<TickResult> {
    let store = GroupStore::new(home.clone())?;
    let mut result = TickResult::default();
    let group = store.load(group_id)?;
    if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return Ok(result);
    }
    let mut state = state::load(&store, group_id)?;
    let previous = state.clone();
    tick_rules(&store, &group, &mut state, &mut result)?;
    if include_unread && group.state == GroupState::Active {
        tick_unread(home, &store, &group, &mut state, &mut result)?;
    }
    if state != previous {
        state::save(&store, group_id, &state)?;
    }
    Ok(result)
}

fn tick_rules(
    store: &GroupStore,
    group: &GroupDoc,
    state: &mut RuntimeState,
    result: &mut TickResult,
) -> io::Result<()> {
    let Some(rules) = group.automation.get("rules").and_then(Value::as_array) else {
        return Ok(());
    };
    let now = Utc::now();
    for rule in rules.iter().filter_map(Value::as_object) {
        if rule.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let id = rule.get("id").and_then(Value::as_str).unwrap_or("");
        let trigger = rule.get("trigger").and_then(Value::as_object);
        if id.is_empty() || !is_due(trigger, state.last_rule.get(id).copied(), now) {
            continue;
        }
        let action = rule.get("action").and_then(Value::as_object);
        let kind = action
            .and_then(|action| action.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("notify");
        match kind {
            "notify" => {
                if let Some(event) = notify_event(group, id, rule, action) {
                    ledger::append(&store.ledger_path(&group.group_id)?, &event)?;
                    result.notifications.push(event);
                }
            }
            "group_state" => {
                if let Some(target) = action
                    .and_then(|action| action.get("state"))
                    .and_then(Value::as_str)
                {
                    result.actions.push(ScheduledAction::GroupState {
                        group_id: group.group_id.clone(),
                        state: target.into(),
                    });
                }
            }
            "actor_control" => {
                let operation = action
                    .and_then(|action| action.get("operation"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let targets = action
                    .and_then(|action| action.get("targets"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                if !operation.is_empty() {
                    result.actions.push(ScheduledAction::ActorControl {
                        group_id: group.group_id.clone(),
                        operation: operation.into(),
                        targets,
                    });
                }
            }
            _ => {}
        }
        state.last_rule.insert(id.into(), now.timestamp());
    }
    Ok(())
}

fn tick_unread(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    state: &mut RuntimeState,
    result: &mut TickResult,
) -> io::Result<()> {
    let threshold = group
        .extra
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| settings.get("unread_nudge_after_seconds"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if threshold <= 0 {
        return Ok(());
    }
    let now = Utc::now().timestamp();
    let actor_ids = actors::visible(group)
        .filter(|actor| actor.enabled)
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let unread = inbox::list_unread_many(home, group, &actor_ids, 1)?;
    for actor_id in actor_ids {
        let Some(message) = unread.get(&actor_id).and_then(|items| items.first()) else {
            continue;
        };
        let sent = DateTime::parse_from_rfc3339(&message.ts)
            .map(|value| value.timestamp())
            .unwrap_or(now);
        let key = format!("{actor_id}:{}", message.id);
        if now - sent < threshold
            || now - state.last_nudge.get(&key).copied().unwrap_or(0) < threshold
        {
            continue;
        }
        let mut event = Event::new("system.notify", &group.group_id);
        event.by = "system".into();
        event.data = json!({
            "kind": "unread_nudge",
            "actor_id": actor_id,
            "to": [actor_id],
            "event_id": message.id,
            "text": "You have an unread collaboration message.",
            "created_at": utc_now(),
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        ledger::append(&store.ledger_path(&group.group_id)?, &event)?;
        result.notifications.push(event);
        state.last_nudge.insert(key, now);
    }
    Ok(())
}

use cccc_contracts::{Event, GroupState};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::io;

use crate::actors;
use crate::automation_render::notify_events;
use crate::automation_schedule::is_due;
use crate::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};

mod state;
use state::RuntimeState;

pub const STANDUP_SNIPPET: &str = "{{interval_minutes}} minutes have passed. Stand-up checkpoint (foreman only).\n\nUse MCP chat for any visible update. Keep this short.";

#[derive(Debug, Clone)]
pub enum ScheduledAction {
    GroupState {
        group_id: String,
        state: String,
        rule_id: String,
        fired_at: i64,
        one_time: bool,
    },
    ActorControl {
        group_id: String,
        operation: String,
        targets: Vec<String>,
        rule_id: String,
        fired_at: i64,
        one_time: bool,
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

pub fn reconcile_rule_state(
    store: &GroupStore,
    group_id: &str,
    previous: &[Value],
    current: &[Value],
) -> io::Result<()> {
    state::reconcile_rules(store, group_id, previous, current)
}

pub fn mark_rule_fired(
    home: &HomeLayout,
    group_id: &str,
    rule_id: &str,
    fired_at: i64,
) -> io::Result<()> {
    let store = GroupStore::new(home.clone())?;
    let mut state = state::load(&store, group_id)?;
    state.last_rule.insert(rule_id.to_owned(), fired_at);
    state::save(&store, group_id, &state)
}

pub fn next_rule_fire_at(
    trigger: Option<&Map<String, Value>>,
    last: Option<i64>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    crate::automation_schedule::next_fire_at(trigger, last, now)
}

pub fn reset_rule_timers_on_resume(home: &HomeLayout, group_id: &str) -> io::Result<()> {
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    let Some(rules) = group.automation.get("rules").and_then(Value::as_array) else {
        return Ok(());
    };
    let now = Utc::now();
    let mut state = state::load(&store, group_id)?;
    let previous = state.clone();
    for rule in rules.iter().filter_map(Value::as_object) {
        if rule.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let id = rule.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let trigger = rule.get("trigger").and_then(Value::as_object);
        let kind = trigger
            .and_then(|trigger| trigger.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let should_reset = match kind {
            "interval" | "cron" => true,
            "at" => trigger
                .and_then(|trigger| trigger.get("at"))
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|scheduled| scheduled <= now),
            _ => false,
        };
        if should_reset {
            state.last_rule.insert(id.to_owned(), now.timestamp());
        }
    }
    if state != previous {
        state::save(&store, group_id, &state)?;
    }
    Ok(())
}

pub fn tick_group(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
) -> io::Result<TickResult> {
    tick_group_inner(home, group_id, include_unread, None)
}

pub fn tick_group_for_delivery_actors(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
    delivery_actor_ids: &HashSet<String>,
) -> io::Result<TickResult> {
    tick_group_inner(home, group_id, include_unread, Some(delivery_actor_ids))
}

fn tick_group_inner(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
    delivery_actor_ids: Option<&HashSet<String>>,
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
    if include_unread && matches!(group.state, GroupState::Active | GroupState::Idle) {
        tick_unread(
            home,
            &store,
            &group,
            delivery_actor_ids,
            &mut state,
            &mut result,
        )?;
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
        if id.is_empty() {
            continue;
        }
        if group.state == GroupState::Idle && id == "standup" {
            continue;
        }
        let trigger = rule.get("trigger").and_then(Value::as_object);
        let last_fired = state.last_rule.get(id).copied();
        let trigger_kind = trigger
            .and_then(|trigger| trigger.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("interval");
        if trigger_kind == "interval" && last_fired.is_none() {
            state.last_rule.insert(id.into(), now.timestamp());
            continue;
        }
        if !is_due(trigger, last_fired, now) {
            continue;
        }
        let action = rule.get("action").and_then(Value::as_object);
        let kind = action
            .and_then(|action| action.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("notify");
        let one_time = trigger_kind == "at";
        let mut completed = false;
        match kind {
            "notify" => {
                let scheduled_at = scheduled_at(trigger, last_fired, now);
                let events = notify_events(group, id, rule, action, &scheduled_at);
                for event in events {
                    ledger::append(&store.ledger_path(&group.group_id)?, &event)?;
                    result.notifications.push(event);
                    completed = true;
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
                        rule_id: id.into(),
                        fired_at: now.timestamp(),
                        one_time,
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
                        rule_id: id.into(),
                        fired_at: now.timestamp(),
                        one_time,
                    });
                }
            }
            _ => {}
        }
        if completed {
            state.last_rule.insert(id.into(), now.timestamp());
        }
    }
    Ok(())
}

fn scheduled_at(
    trigger: Option<&serde_json::Map<String, Value>>,
    last_fired: Option<i64>,
    now: DateTime<Utc>,
) -> String {
    let kind = trigger
        .and_then(|trigger| trigger.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("interval");
    let timestamp = match kind {
        "interval" => last_fired
            .zip(
                trigger
                    .and_then(|trigger| trigger.get("every_seconds"))
                    .and_then(Value::as_i64),
            )
            .and_then(|(last, seconds)| DateTime::from_timestamp(last + seconds, 0)),
        "at" => trigger
            .and_then(|trigger| trigger.get("at"))
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc)),
        "cron" => DateTime::from_timestamp(now.timestamp().div_euclid(60) * 60, 0),
        _ => None,
    };
    timestamp.map_or_else(String::new, |value| value.to_rfc3339())
}

fn tick_unread(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    delivery_actor_ids: Option<&HashSet<String>>,
    _state: &mut RuntimeState,
    result: &mut TickResult,
) -> io::Result<()> {
    let mail_after = delivery_timing_value(group, "mail_notice_after_seconds", 1_800);
    let reply_after = delivery_timing_value(group, "reply_notice_after_seconds", 900);
    if mail_after <= 0 && reply_after <= 0 {
        return Ok(());
    }
    let ledger_path = store.ledger_path(&group.group_id)?;
    let events = ledger::read_all(&ledger_path)?;
    let positions = events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let generations = inbox::actor_generation_positions(&events);
    let cursors = inbox::cursors(home, &group.group_id)?;
    let now = Utc::now().timestamp();
    let resume_at = events.iter().rev().find_map(|event| {
        let resumed = event.kind == "group.start"
            || (event.kind == "group.set_state"
                && matches!(
                    event.data.get("new_state").and_then(Value::as_str),
                    Some("active" | "idle")
                ));
        resumed.then(|| timestamp(&event.ts, now))
    });

    let mut mail_reads = HashMap::<String, Vec<(usize, usize)>>::new();
    let mut replies = HashMap::<(String, String), usize>::new();
    let mut cancelled = HashSet::<String>::new();
    let mut deliveries = HashMap::<(String, String), (String, i64, usize)>::new();
    let mut actor_resumes = HashMap::<String, i64>::new();
    let mut mail_claims = HashMap::<(String, String), Vec<Vec<String>>>::new();
    let mut reply_claims = HashSet::<(String, String, String)>::new();
    for (position, event) in events.iter().enumerate() {
        match event.kind.as_str() {
            "actor.start" | "actor.restart" | "actor.new_session" => {
                if let Some(actor_id) = event.data.get("actor_id").and_then(Value::as_str) {
                    actor_resumes.insert(actor_id.to_owned(), timestamp(&event.ts, now));
                }
            }
            "mail.read" => {
                if let (Some(actor_id), Some(boundary_id)) = (
                    event.data.get("actor_id").and_then(Value::as_str),
                    event.data.get("event_id").and_then(Value::as_str),
                ) && let Some(boundary_position) = positions.get(boundary_id)
                {
                    mail_reads
                        .entry(actor_id.to_owned())
                        .or_default()
                        .push((position, *boundary_position));
                }
            }
            "chat.message" => {
                if let Some(source) = event.data.get("reply_to").and_then(Value::as_str) {
                    replies
                        .entry((source.to_owned(), event.by.clone()))
                        .or_insert(position);
                }
            }
            "chat.reply_request.cancelled" => {
                if let Some(source) = event.data.get("source_event_id").and_then(Value::as_str) {
                    cancelled.insert(source.to_owned());
                }
            }
            "runtime.delivery" => {
                if let (Some(source), Some(actor_id), Some(state)) = (
                    event.data.get("source_event_id").and_then(Value::as_str),
                    event.data.get("actor_id").and_then(Value::as_str),
                    event.data.get("state").and_then(Value::as_str),
                ) {
                    deliveries.insert(
                        (source.to_owned(), actor_id.to_owned()),
                        (state.to_owned(), timestamp(&event.ts, now), position),
                    );
                }
            }
            "system.notify" => {
                let kind = event
                    .data
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !matches!(kind, "mail_notice" | "reply_notice") {
                    continue;
                }
                let context = event.data.get("context").and_then(Value::as_object);
                let actor_id = context
                    .and_then(|value| value.get("actor_id"))
                    .and_then(Value::as_str)
                    .or_else(|| event.data.get("target_actor_id").and_then(Value::as_str))
                    .unwrap_or_default();
                let created_at = context
                    .and_then(|value| value.get("actor_created_at"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let source_ids = context
                    .and_then(|value| value.get("source_event_ids"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if kind == "mail_notice" {
                    mail_claims
                        .entry((actor_id.into(), created_at.into()))
                        .or_default()
                        .push(source_ids);
                } else {
                    reply_claims.extend(
                        source_ids
                            .into_iter()
                            .map(|source| (actor_id.into(), created_at.into(), source)),
                    );
                }
            }
            _ => {}
        }
    }

    let resolution_position = |actor_id: &str, source_event_id: &str| {
        let source_position = positions.get(source_event_id).copied()?;
        let read_position = mail_reads.get(actor_id).and_then(|facts| {
            facts.iter().find_map(|(fact_position, boundary_position)| {
                (*boundary_position >= source_position).then_some(*fact_position)
            })
        });
        let reply_position = replies
            .get(&(source_event_id.to_owned(), actor_id.to_owned()))
            .copied();
        let delivery_position = deliveries
            .get(&(source_event_id.to_owned(), actor_id.to_owned()))
            .and_then(|(state, _, fact_position)| {
                matches!(state.as_str(), "accepted" | "ambiguous").then_some(*fact_position)
            });
        [read_position, reply_position, delivery_position]
            .into_iter()
            .flatten()
            .min()
    };

    for actor in actors::visible(group).filter(|actor| {
        actor.enabled && delivery_actor_ids.is_none_or(|actor_ids| actor_ids.contains(&actor.id))
    }) {
        let generation = generations.get(&actor.id).copied().unwrap_or(0);
        let cursor_position = cursors
            .get(&actor.id)
            .and_then(|event_id| positions.get(event_id))
            .copied();
        let mut mail_pending = Vec::<&Event>::new();
        let mut reply_due = Vec::<&Event>::new();
        for (position, source) in events.iter().enumerate().skip(generation) {
            if source.kind != "chat.message"
                || source.by == actor.id
                || !inbox::is_for_actor(group, source, &actor.id)
            {
                continue;
            }
            let mode = source
                .data
                .get("message_mode")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let read = cursor_position.is_some_and(|cursor| cursor >= position);
            let replied = replies.contains_key(&(source.id.clone(), actor.id.clone()));
            let delivery = deliveries.get(&(source.id.clone(), actor.id.clone()));
            if mode == "mail" {
                let recipients = source
                    .data
                    .get("to")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>();
                let broadcast_like = recipients.is_empty()
                    || recipients
                        .iter()
                        .any(|recipient| matches!(*recipient, "@all" | "@peers" | "@foreman"));
                if !broadcast_like
                    && !read
                    && !replied
                    && !delivery.is_some_and(|(state, _, _)| {
                        matches!(state.as_str(), "accepted" | "ambiguous")
                    })
                {
                    mail_pending.push(source);
                }
                continue;
            }
            if mode != "request_reply"
                || replied
                || cancelled.contains(&source.id)
                || reply_claims.contains(&(
                    actor.id.clone(),
                    actor.created_at.clone(),
                    source.id.clone(),
                ))
            {
                continue;
            }
            let Some(started_at) = delivery
                .filter(|(state, _, _)| state == "accepted")
                .map(|(_, at, _)| *at)
            else {
                continue;
            };
            let started_at = resume_at.map_or(started_at, |resume| started_at.max(resume));
            if reply_after > 0 && now - started_at >= reply_after {
                reply_due.push(source);
            }
        }

        if mail_after > 0 && !mail_pending.is_empty() {
            let pending_ids = mail_pending
                .iter()
                .map(|event| event.id.clone())
                .collect::<HashSet<_>>();
            let active_claim = mail_claims
                .get(&(actor.id.clone(), actor.created_at.clone()))
                .into_iter()
                .flatten()
                .rev()
                .any(|claimed| {
                    let claimed = claimed
                        .iter()
                        .filter(|source| positions.contains_key(*source))
                        .collect::<Vec<_>>();
                    if claimed.is_empty() {
                        return true;
                    }
                    let resolutions = claimed
                        .iter()
                        .map(|source| resolution_position(&actor.id, source))
                        .collect::<Vec<_>>();
                    if resolutions.iter().any(Option::is_none) {
                        return true;
                    }
                    let closure_position = resolutions.into_iter().flatten().max().unwrap_or(0);
                    pending_ids.iter().any(|source| {
                        positions
                            .get(source)
                            .is_some_and(|position| *position <= closure_position)
                    })
                });
            let first_at = timestamp(&mail_pending[0].ts, now);
            let first_at = resume_at.map_or(first_at, |resume| first_at.max(resume));
            let first_at = actor_resumes
                .get(&actor.id)
                .map_or(first_at, |resume| first_at.max(*resume));
            if !active_claim && now - first_at >= mail_after {
                let event = notice_event(
                    group,
                    actor,
                    "mail_notice",
                    "Mail waiting",
                    &format!(
                        "You have {} Mail item(s) waiting. Call cccc_inbox_read when appropriate.",
                        mail_pending.len()
                    ),
                    mail_pending.iter().map(|event| event.id.clone()).collect(),
                );
                ledger::append(&ledger_path, &event)?;
                result.notifications.push(event);
            }
        }
        if !reply_due.is_empty() {
            let event = notice_event(
                group,
                actor,
                "reply_notice",
                "Reply requested",
                &format!(
                    "{} message(s) still need a concrete reply. Use cccc_message_history if needed, then cccc_message_reply.",
                    reply_due.len()
                ),
                reply_due.iter().map(|event| event.id.clone()).collect(),
            );
            ledger::append(&ledger_path, &event)?;
            result.notifications.push(event);
        }
    }
    Ok(())
}

fn delivery_timing_value(group: &GroupDoc, key: &str, default: i64) -> i64 {
    group
        .extra
        .get("delivery")
        .and_then(Value::as_object)
        .and_then(|delivery| delivery.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(default)
        .max(0)
}

fn timestamp(value: &str, default: i64) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp())
        .unwrap_or(default)
}

fn notice_event(
    group: &GroupDoc,
    actor: &cccc_contracts::Actor,
    kind: &str,
    title: &str,
    message: &str,
    source_event_ids: Vec<String>,
) -> Event {
    let mut event = Event::new("system.notify", &group.group_id);
    event.by = "system".into();
    event.data = json!({
        "kind":kind,
        "priority":"normal",
        "title":title,
        "message":message,
        "target_actor_id":actor.id,
        "related_event_id":source_event_ids.first().cloned().unwrap_or_default(),
        "im_visibility":"internal",
        "context":{
            "actor_id":actor.id,
            "actor_created_at":actor.created_at,
            "source_event_ids":source_event_ids,
            "count":source_event_ids.len(),
        },
    })
    .as_object()
    .cloned()
    .expect("reminder data");
    event
}

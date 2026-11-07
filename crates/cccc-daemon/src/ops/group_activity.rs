//! Per-member counts from the ledger and the task board, for a team review.
//! They are leads: what a number stands for has to be read before concluding
//! anything about a member. An Actor counts only the messages it may read, so
//! the counts reveal no private exchange between the user and another member.

use std::collections::HashMap;

use cccc_contracts::{DaemonRequest, Event};
use cccc_core::context::ContextStore;
use cccc_core::{GroupDoc, HomeLayout, actors, inbox, ledger, permissions};
use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

const DEFAULT_DAYS: i64 = 14;
const MAX_DAYS: i64 = 365;

#[derive(Default)]
struct Counts {
    messages_sent: u64,
    messages_to_user: u64,
    scheduled_notices: u64,
    last_message_at: String,
}

impl Counts {
    fn record_message(&mut self, event: &Event) {
        self.messages_sent += 1;
        self.last_message_at.clone_from(&event.ts);
        let to_user = event
            .data
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|to| to.iter().any(|item| item.as_str() == Some("user")));
        self.messages_to_user += u64::from(to_user);
    }
}

pub(super) fn activity(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = super::context::load_group(home, &group_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    permissions::require_group(&group, &by).map_err(OpError::invalid)?;
    let days = window_days(request)?;
    let since = (Utc::now() - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Secs, true);

    let path = store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let counts = ledger::inspect_status(&path, |events, _, generations| {
        count_events(&group, events, generations, &by, &since)
    })
    .map_err(OpError::io)?;
    let context = ContextStore::new(home.clone())
        .and_then(|contexts| contexts.load(&group.group_id))
        .map_err(OpError::io)?;

    let members = actors::visible(&group)
        .map(|actor| {
            let tasks_open = context
                .tasks
                .iter()
                .filter(|task| task.get("assignee").and_then(Value::as_str) == Some(&actor.id))
                .filter_map(|task| task.get("status").and_then(Value::as_str))
                .filter(|status| matches!(*status, "planned" | "active"))
                .count();
            let mut row = counts_json(counts.get(&actor.id));
            row["actor_id"] = Value::String(actor.id.clone());
            row["tasks_open"] = json!(tasks_open);
            row
        })
        .collect::<Vec<_>>();
    object(json!({
        "days": days,
        "since": since,
        "user": counts_json(counts.get("user")),
        "members": members,
    }))
}

fn window_days(request: &DaemonRequest) -> Result<i64, OpError> {
    let Some(value) = request.args.get("days") else {
        return Ok(DEFAULT_DAYS);
    };
    value
        .as_i64()
        .filter(|days| (1..=MAX_DAYS).contains(days))
        .ok_or_else(|| {
            OpError::new(
                "invalid_args",
                format!("days must be an integer from 1 to {MAX_DAYS}"),
            )
        })
}

fn count_events(
    group: &GroupDoc,
    events: &[Event],
    generations: &HashMap<String, usize>,
    reader: &str,
    since: &str,
) -> HashMap<String, Counts> {
    // An event before an Actor's current generation belongs to a former registration.
    let current = |actor_id: &str, index: usize| {
        generations
            .get(actor_id)
            .is_none_or(|generation| index >= *generation)
    };
    let readable = |event: &Event| {
        reader == "user" || event.by == reader || inbox::is_for_actor(group, event, reader)
    };
    let mut counts = HashMap::<String, Counts>::new();
    for (index, event) in events.iter().enumerate() {
        if event.ts.as_str() < since || !current(reader, index) {
            continue;
        }
        match event.kind.as_str() {
            "chat.message" if current(&event.by, index) && readable(event) => counts
                .entry(event.by.clone())
                .or_default()
                .record_message(event),
            "system.notify"
                if event.data.get("kind").and_then(Value::as_str) == Some("automation") =>
            {
                if let Some(target) = event
                    .data
                    .get("target_actor_id")
                    .and_then(Value::as_str)
                    .filter(|target| current(target, index))
                {
                    counts
                        .entry(target.to_owned())
                        .or_default()
                        .scheduled_notices += 1;
                }
            }
            _ => {}
        }
    }
    counts
}

fn counts_json(counts: Option<&Counts>) -> Value {
    let default = Counts::default();
    let counts = counts.unwrap_or(&default);
    json!({
        "messages_sent": counts.messages_sent,
        "messages_to_user": counts.messages_to_user,
        "scheduled_notices": counts.scheduled_notices,
        "last_message_at": counts.last_message_at,
    })
}

use cccc_contracts::{Event, GroupState};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
#[cfg(test)]
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;

use crate::actors;
use crate::automation_render::notify_events;
use crate::automation_schedule::is_due;
use crate::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};

mod state;
use state::{RunRecord, RuntimeState};

/// The built-in stand-up text. It names no interval, since a rule may also run
/// on a schedule, where `{{interval_minutes}}` is 0.
pub const STANDUP_SNIPPET: &str = "Stand-up for the foreman: a short look at how the team works \
against the user's current goals, not just at task status. Act on what your authority covers and \
do not ask members for status reports. If the user asked to hear about this check, report \
briefly even when nothing needs action; otherwise end without a message when nothing needs \
anyone.\n\
\n\
What you need may already be in view. If not: cccc_task(action=\"list\") lists open tasks in \
pages, with owner, waiting_on and updated_at where set; cccc_message_history(mode=\"all\", \
limit=10) shows recent messages you can see, and adding sender=\"user\" shows only the user's. \
Neither shows everything, so do not claim you saw everything.\n\
\n\
1. Direction: does the open work still serve the user's current goals and outstanding requests, \
including their latest corrections, in the order they care about? The brief may be out of date; \
go by the user's own words, and check with the user when unsure rather than dropping work they \
approved. Look for work that is off course or done for its own sake, and for work that is \
blocked, waiting, unowned or not moving. An old updated_at alone does not prove work stopped: \
find out why before you unblock or reassign it, and never close a task for waiting or age alone.\n\
2. Friction: where did members wait on each other, redo or duplicate work, hand off unclearly or \
misread each other, and what caused it? Is anyone overloaded or left without work, and do the \
members, their setup and the team's size still fit the work? Judge from the work itself, not \
from an idle state, a message count or an old timestamp.\n\
3. The user's worries: what has the user had to say twice, or kept coming back to, and did a \
later case of the same kind go better? A worry may have no task yet. If a change has an open \
optimization task and work it applies to came up, read the task by task_id and add the \
result to its notes, keeping the existing notes and passing the if_task_rev you read. No later \
case, or the user saying nothing, does not prove it improved.\n\
4. A better way: if a practical change to how the team works would clearly pay off, propose it \
with its evidence. Do not invent a problem or a proposal.\n\
\n\
Fix causes within your authority, tell the user about routine changes, and message only a member \
who has to act. Bring the user at most one decision, on a question card, such as a standing rule \
or a member change, and do not repeat what is already in front of them unless it changed. A \
change the user adopts, or one within your authority that is worth checking later, becomes an \
optimization task with its evidence, when it applies, what to look at next time and how to undo \
it; a one-off fix needs none. For a message addressed only to you, share only what the user \
authorized and the team's work needs, in your own words, and honor any confidentiality limits. \
Do not include its ID unless the user specifically agreed.\n\
\n\
When a problem needs a deeper look across rounds, add a line starting with \"Review:\" to the \
notes of a related open optimization task, keeping the existing notes and passing the \
if_task_rev you read; if there is none, create one planned optimization task whose title starts \
with \"Review:\", whose notes start by saying it authorizes no change, and which holds only facts \
the Group can see or the user allowed you to share. When the work already sits in another task \
or handoff, point to it instead of copying it. Something merely noticed needs no task. You may \
propose a scoped team review, but do not run the review \
skill from here. A Group that creates few tasks may wait long for the review's task reminder: only \
with current, reliable information that the skill is available to you, and when a review would \
clearly pay off, your one decision may be whether to run it now; when unsure, do not ask. Do not \
check capability state just to decide this, and do not ask when the user has already asked for a \
review.";

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
    pub completed_rules: Vec<(String, String)>,
    /// A later rule append or state write failed after earlier notices were
    /// committed. Those notices must still be handed to the delivery path.
    pub rule_failure: Option<io::Error>,
    /// Failure while discovering or appending Mail/reply notices. Retained
    /// alongside committed notifications and prepared rule actions so callers
    /// can still apply those results. Unwritten notices remain due for a later pass.
    pub unread_failure: Option<io::Error>,
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
        result.completed_rules.extend(group_result.completed_rules);
        if result.rule_failure.is_none() {
            result.rule_failure = group_result.rule_failure;
        }
        if result.unread_failure.is_none() {
            result.unread_failure = group_result.unread_failure;
        }
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

/// A manual run's committed notifications, retained even if a later append or
/// state write failed. Callers must deliver committed events before reporting
/// the error; a committed notice cannot be undone.
pub struct RuleNotificationResult {
    pub notifications: Vec<Event>,
    pub failure: Option<io::Error>,
}

pub fn notify_rule_now(
    home: &HomeLayout,
    group: &GroupDoc,
    rule_id: &str,
    rule: &Map<String, Value>,
) -> io::Result<RuleNotificationResult> {
    notify_rule_now_with(home, group, rule_id, rule, &mut ledger::append)
}

fn notify_rule_now_with(
    home: &HomeLayout,
    group: &GroupDoc,
    rule_id: &str,
    rule: &Map<String, Value>,
    append: &mut impl FnMut(&Path, &Event) -> io::Result<()>,
) -> io::Result<RuleNotificationResult> {
    let store = GroupStore::new(home.clone())?;
    let mut state = state::load(&store, &group.group_id)?;
    let now = Utc::now();
    let events = notify_events(
        group,
        rule_id,
        rule,
        rule.get("action").and_then(Value::as_object),
        &now.to_rfc3339(),
        "manual",
    );
    let path = store.ledger_path(&group.group_id)?;
    let mut result = append_rule_notifications(&path, events, append);
    if let Some(first) = result.notifications.first() {
        let at = DateTime::parse_from_rfc3339(&first.ts)
            .map(|timestamp| timestamp.timestamp())
            .unwrap_or_else(|_| now.timestamp());
        state.last_run.insert(
            rule_id.to_owned(),
            RunRecord {
                at,
                kind: "manual".into(),
            },
        );
        if let Err(error) = state::save(&store, &group.group_id, &state) {
            let message = match result.failure.take() {
                Some(earlier) => format!("{earlier}; failed to record last run: {error}"),
                None => format!("failed to record last run: {error}"),
            };
            result.failure = Some(io::Error::other(message));
        }
    }
    Ok(result)
}

fn append_rule_notifications(
    path: &Path,
    events: Vec<Event>,
    append: &mut impl FnMut(&Path, &Event) -> io::Result<()>,
) -> RuleNotificationResult {
    let mut result = RuleNotificationResult {
        notifications: Vec::new(),
        failure: None,
    };
    for event in events {
        if let Err(error) = append(path, &event) {
            result.failure = Some(error);
            break;
        }
        result.notifications.push(event);
    }
    result
}

pub fn next_rule_fire_at(
    trigger: Option<&Map<String, Value>>,
    last: Option<i64>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    crate::automation_schedule::next_fire_at(trigger, last, now)
}

/// Validate a cron trigger expression, returning the parse error text on failure.
/// Day-of-week fields are interpreted with POSIX numbering (0 or 7 = Sunday).
pub fn cron_expression_error(raw: &str) -> Option<String> {
    crate::automation_schedule::parse_cron_schedule(raw)
        .err()
        .map(|error| error.to_string())
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
        if trigger
            .and_then(|t| t.get("max_runs"))
            .and_then(Value::as_u64)
            .is_some_and(|limit| state.fired_count.get(id).copied().unwrap_or(0) >= limit)
        {
            continue;
        }
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
    tick_group_inner(home, group_id, include_unread, None, &HashSet::new())
}

pub fn tick_group_for_delivery_actors(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
    delivery_actor_ids: &HashSet<String>,
) -> io::Result<TickResult> {
    tick_group_inner(
        home,
        group_id,
        include_unread,
        Some(delivery_actor_ids),
        &HashSet::new(),
    )
}

/// Like [`tick_group_for_delivery_actors`], with the actors known to be idle:
/// waiting Mail reaches them after the short idle delay instead of the
/// interruption-avoiding delay used while they work or their state is unknown.
pub fn tick_group_with_idle_actors(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
    delivery_actor_ids: &HashSet<String>,
    idle_actor_ids: &HashSet<String>,
) -> io::Result<TickResult> {
    tick_group_inner(
        home,
        group_id,
        include_unread,
        Some(delivery_actor_ids),
        idle_actor_ids,
    )
}

/// Announce waiting Mail right away to `idle_actor_ids`, the recipients of a
/// Mail just written that are idle now, instead of on the next unread tick.
/// Only Mail notices are issued, under the tick's rules: one notice per unread
/// batch, `deliver_by` at the busy delay, and nothing when early notices are
/// off (`mail_notice_idle_after_seconds` 0). Recipients that are busy keep the
/// tick's delays.
pub fn notify_idle_mail(
    home: &HomeLayout,
    group_id: &str,
    idle_actor_ids: &HashSet<String>,
) -> io::Result<TickResult> {
    let mut result = TickResult::default();
    if idle_actor_ids.is_empty() {
        return Ok(result);
    }
    let store = GroupStore::new(home.clone())?;
    let group = store.load(group_id)?;
    if matches!(group.state, GroupState::Active | GroupState::Idle) {
        tick_unread(
            home,
            &store,
            &group,
            Some(idle_actor_ids),
            idle_actor_ids,
            UnreadPass::IdleMailNow,
            &mut result,
        )?;
    }
    Ok(result)
}

fn tick_group_inner(
    home: &HomeLayout,
    group_id: &str,
    include_unread: bool,
    delivery_actor_ids: Option<&HashSet<String>>,
    idle_actor_ids: &HashSet<String>,
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
    // Save before issuing unread notices: a notice written to the ledger counts
    // as issued, so no error may follow it and drop the result carrying it.
    if state != previous
        && let Err(error) = state::save(&store, group_id, &state)
    {
        retain_committed_after_state_failure(&mut result, error)?;
    }
    if include_unread && matches!(group.state, GroupState::Active | GroupState::Idle) {
        tick_unread(
            home,
            &store,
            &group,
            delivery_actor_ids,
            idle_actor_ids,
            UnreadPass::Scheduled,
            &mut result,
        )?;
    }
    Ok(result)
}

fn retain_committed_after_state_failure(
    result: &mut TickResult,
    error: io::Error,
) -> io::Result<()> {
    if result.notifications.is_empty() {
        return Err(error);
    }
    // Actions have not been executed yet. Without saved rule state, leave them
    // for the next tick instead of making a lifecycle change that may repeat.
    result.actions.clear();
    result.completed_rules.clear();
    result.rule_failure = Some(match result.rule_failure.take() {
        Some(previous) => io::Error::other(format!("{previous}; rule state save failed: {error}")),
        None => error,
    });
    Ok(())
}

fn tick_rules(
    store: &GroupStore,
    group: &GroupDoc,
    state: &mut RuntimeState,
    result: &mut TickResult,
) -> io::Result<()> {
    tick_rules_with(store, group, state, result, &mut ledger::append)
}

fn tick_rules_with(
    store: &GroupStore,
    group: &GroupDoc,
    state: &mut RuntimeState,
    result: &mut TickResult,
    append: &mut impl FnMut(&Path, &Event) -> io::Result<()>,
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
        let max_runs = trigger
            .and_then(|t| t.get("max_runs"))
            .and_then(Value::as_u64);
        if max_runs.is_some_and(|limit| state.fired_count.get(id).copied().unwrap_or(0) >= limit) {
            result
                .completed_rules
                .push((group.group_id.clone(), id.into()));
            continue;
        }
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
        let mut stop_after_rule = false;
        match kind {
            "notify" => {
                let scheduled_at = scheduled_at(trigger, last_fired, now);
                let events = notify_events(group, id, rule, action, &scheduled_at, "scheduled");
                let outcome =
                    append_rule_notifications(&store.ledger_path(&group.group_id)?, events, append);
                completed = !outcome.notifications.is_empty();
                if let Some(first) = outcome.notifications.first() {
                    let at = DateTime::parse_from_rfc3339(&first.ts)
                        .map(|timestamp| timestamp.timestamp())
                        .unwrap_or_else(|_| now.timestamp());
                    state.last_run.insert(
                        id.into(),
                        RunRecord {
                            at,
                            kind: "scheduled".into(),
                        },
                    );
                }
                result.notifications.extend(outcome.notifications);
                if let Some(error) = outcome.failure {
                    if result.notifications.is_empty() && result.actions.is_empty() {
                        return Err(error);
                    }
                    result.rule_failure = Some(error);
                    stop_after_rule = true;
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
            if matches!(trigger_kind, "interval" | "cron")
                && let Some(limit) = max_runs
            {
                let count = state.fired_count.entry(id.into()).or_default();
                *count = count.saturating_add(1);
                if *count >= limit {
                    result
                        .completed_rules
                        .push((group.group_id.clone(), id.into()));
                }
            }
        }
        if stop_after_rule {
            break;
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

/// The periodic unread tick, or the pass that runs as Mail is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnreadPass {
    Scheduled,
    IdleMailNow,
}

fn tick_unread(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    delivery_actor_ids: Option<&HashSet<String>>,
    idle_actor_ids: &HashSet<String>,
    pass: UnreadPass,
    result: &mut TickResult,
) -> io::Result<()> {
    let due = match due_unread_notices(home, store, group, delivery_actor_ids, idle_actor_ids, pass)
    {
        Ok(due) => due,
        Err(error) if result.notifications.is_empty() && result.actions.is_empty() => {
            return Err(error);
        }
        Err(error) => {
            result.unread_failure = Some(error);
            return Ok(());
        }
    };
    if let Some((ledger_path, notices)) = due {
        issue_notices(&ledger_path, notices, result, &mut ledger::append);
    }
    Ok(())
}

/// The Mail and reply notices this pass should issue, and the ledger they go
/// to; `None` when the pass has nothing to look at.
fn due_unread_notices(
    home: &HomeLayout,
    store: &GroupStore,
    group: &GroupDoc,
    delivery_actor_ids: Option<&HashSet<String>>,
    idle_actor_ids: &HashSet<String>,
    pass: UnreadPass,
) -> io::Result<Option<(std::path::PathBuf, Vec<Event>)>> {
    let mail_after = delivery_timing_value(group, "mail_notice_after_seconds", 1_800);
    let mail_idle_after = delivery_timing_value(group, "mail_notice_idle_after_seconds", 60);
    if pass == UnreadPass::IdleMailNow && mail_idle_after <= 0 {
        return Ok(None);
    }
    // Reply notices stay with the periodic tick.
    let reply_after = match pass {
        UnreadPass::Scheduled => delivery_timing_value(group, "reply_notice_after_seconds", 900),
        UnreadPass::IdleMailNow => 0,
    };
    if mail_after <= 0 && reply_after <= 0 {
        return Ok(None);
    }
    let eligible = actors::visible(group)
        .filter(|actor| {
            actor.enabled && delivery_actor_ids.is_none_or(|ids| ids.contains(&actor.id))
        })
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return Ok(None);
    }
    let ledger_path = store.ledger_path(&group.group_id)?;
    let notices = inbox::projection::due(
        home,
        group,
        &eligible,
        MailDelays {
            busy: mail_after,
            idle: mail_idle_after,
            idle_actor_ids,
            now: pass == UnreadPass::IdleMailNow,
        },
        reply_after,
    )?;
    #[cfg(test)]
    {
        let cursors = inbox::cursors(home, &group.group_id)?;
        let expected = ledger::inspect(&ledger_path, |events, positions| {
            unread_notices(
                group,
                &eligible,
                events,
                positions,
                &cursors,
                MailDelays {
                    busy: mail_after,
                    idle: mail_idle_after,
                    idle_actor_ids,
                    now: pass == UnreadPass::IdleMailNow,
                },
                reply_after,
            )
        })?;
        let signature = |events: &[Event]| {
            events
                .iter()
                .map(|event| (&event.kind, &event.by, &event.data))
                .map(|parts| serde_json::to_value(parts).expect("notice signature"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            signature(&notices),
            signature(&expected),
            "compact notice reducer disagrees with the original ordered reducer"
        );
    }
    Ok(Some((ledger_path, notices)))
}

/// Write each notice and hand it to delivery. A written notice is issued for
/// good, so it goes to delivery even when a later one fails; returning the
/// error instead would strand it. The rest stay unwritten for a later pass.
fn issue_notices(
    ledger_path: &std::path::Path,
    notices: Vec<Event>,
    result: &mut TickResult,
    append: &mut impl FnMut(&std::path::Path, &Event) -> io::Result<()>,
) {
    for event in notices {
        if let Err(error) = append(ledger_path, &event) {
            result.unread_failure = Some(error);
            break;
        }
        result.notifications.push(event);
    }
}

/// How long unread Mail waits before its notice. Mail exists so senders do not
/// interrupt work: while an actor works, or its state is unknown, the notice
/// waits `busy`; once the actor is idle it only waits `idle`, which batches a
/// burst of Mail into one notice. `busy <= 0` turns Mail notices off; `idle <= 0`
/// turns early notices off. With `now`, an idle actor is notified at once: the
/// pass that runs as Mail is written.
pub(crate) struct MailDelays<'a> {
    pub(crate) busy: i64,
    idle: i64,
    idle_actor_ids: &'a HashSet<String>,
    now: bool,
}

impl MailDelays<'_> {
    pub(crate) fn for_actor(&self, actor_id: &str) -> i64 {
        if self.busy > 0 && self.idle > 0 && self.idle_actor_ids.contains(actor_id) {
            if self.now {
                0
            } else {
                self.idle.min(self.busy)
            }
        } else {
            self.busy
        }
    }
}

#[cfg(test)]
type ReferenceMailClaims = HashMap<(String, String), Vec<(String, i64, Vec<String>)>>;

#[cfg(test)]
fn unread_notices(
    group: &GroupDoc,
    eligible: &[&cccc_contracts::Actor],
    events: &[Event],
    positions: &HashMap<String, usize>,
    cursors: &BTreeMap<String, String>,
    mail_delays: MailDelays<'_>,
    reply_after: i64,
) -> Vec<Event> {
    let mut notices = Vec::new();
    let generations = inbox::actor_generation_positions(events);
    let now = Utc::now().timestamp();
    let mail_repeat = delivery_timing_value(group, "mail_notice_repeat_seconds", 0);
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
    // Each Mail notice, keyed by its own event id: a withdrawn notice was never
    // sent and must not stop the next one.
    let mut mail_claims = ReferenceMailClaims::new();
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
                        .push((event.id.clone(), timestamp(&event.ts, now), source_ids));
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

    for actor in eligible {
        let mail_after = mail_delays.for_actor(&actor.id);
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
                    && crate::mail_pending::awaiting(
                        read,
                        replied,
                        delivery.map(|(state, _, _)| state.as_str()),
                    )
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

        if mail_delays.busy > 0 && !mail_pending.is_empty() {
            let pending_ids = mail_pending
                .iter()
                .map(|event| event.id.clone())
                .collect::<HashSet<_>>();
            let active_claim = mail_claims
                .get(&(actor.id.clone(), actor.created_at.clone()))
                .into_iter()
                .flatten()
                .rev()
                .filter(|(notice_id, claimed_at, _)| {
                    match deliveries
                        .get(&(notice_id.clone(), actor.id.clone()))
                        .map(|(state, _, _)| state.as_str())
                    {
                        Some("withdrawn") => false,
                        Some("accepted" | "ambiguous") => {
                            mail_repeat <= 0 || now - claimed_at < mail_repeat
                        }
                        _ => true,
                    }
                })
                .any(|(_, _, claimed)| {
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
                let mut event = notice_event(
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
                if mail_after < mail_delays.busy
                    && let Some(deadline) = DateTime::from_timestamp(first_at + mail_delays.busy, 0)
                    && let Some(context) =
                        event.data.get_mut("context").and_then(Value::as_object_mut)
                {
                    // Sent early because the Actor looked idle: delivery may hold it
                    // while the Actor turns out busy, but no later than the notice
                    // the busy delay would have sent anyway.
                    context.insert("deliver_by".into(), json!(deadline.to_rfc3339()));
                }
                notices.push(event);
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
            notices.push(event);
        }
    }
    notices
}

pub(crate) fn delivery_timing_value(group: &GroupDoc, key: &str, default: i64) -> i64 {
    group
        .extra
        .get("delivery")
        .and_then(Value::as_object)
        .and_then(|delivery| delivery.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(default)
        .max(0)
}

pub(crate) fn timestamp(value: &str, default: i64) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp())
        .unwrap_or(default)
}

pub(crate) fn notice_event(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The text asks the stand-up to hand a deeper look to the team review
    /// rather than run it, and not to probe capability state to decide.
    #[test]
    fn the_standup_leaves_the_review_to_the_user_and_its_reminder() {
        for promise in [
            "share only what the user authorized and the team's work needs",
            "honor any confidentiality limits",
            "Do not include its ID unless the user specifically agreed.",
            "facts the Group can see or the user allowed you to share",
            "a one-off fix needs none",
            "add a line starting with \"Review:\"",
            "whose title starts with \"Review:\"",
            "start by saying it authorizes no change",
            "point to it instead of copying it",
            "when unsure, do not ask",
            "Something merely noticed needs no task.",
            "do not run the review skill from here",
            "Do not check capability state just to decide this",
        ] {
            assert!(STANDUP_SNIPPET.contains(promise), "{promise}");
        }
    }

    fn two_recipient_rule() -> (tempfile::TempDir, HomeLayout, GroupStore, GroupDoc, Value) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("manual test", "").expect("group");
        let rule = json!({
            "id":"reminder", "enabled":true, "to":["first", "second"],
            "trigger":{"kind":"interval", "every_seconds":60, "max_runs":2},
            "action":{"kind":"notify", "message":"check in"}
        });
        store
            .mutate(&group.group_id, |group| {
                actors::add(group, cccc_contracts::Actor::new("first"))?;
                actors::add(group, cccc_contracts::Actor::new("second"))?;
                group
                    .automation
                    .insert("rules".into(), json!([rule.clone()]));
                Ok(())
            })
            .expect("configure");
        let group = store.load(&group.group_id).expect("load group");
        (temp, home, store, group, rule)
    }

    #[test]
    fn manual_partial_append_keeps_committed_notice_and_records_the_run() {
        let (_temp, home, store, group, rule) = two_recipient_rule();
        let mut writes = 0;
        let result = notify_rule_now_with(
            &home,
            &group,
            "reminder",
            rule.as_object().expect("rule"),
            &mut |path, event| {
                writes += 1;
                if writes == 2 {
                    Err(io::Error::other("disk full"))
                } else {
                    ledger::append(path, event)
                }
            },
        )
        .expect("committed results");
        assert_eq!(result.notifications.len(), 1);
        assert!(result.failure.is_some());
        assert_eq!(
            result.notifications[0].data["context"]["run_kind"],
            "manual"
        );
        let saved = state::load(&store, &group.group_id).expect("saved state");
        assert_eq!(saved.last_run["reminder"].kind, "manual");
        assert!(!saved.last_rule.contains_key("reminder"));
        assert!(!saved.fired_count.contains_key("reminder"));
        let ledger =
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger")).expect("events");
        assert!(
            ledger
                .iter()
                .any(|event| event.id == result.notifications[0].id)
        );
    }

    #[test]
    fn scheduled_partial_append_keeps_committed_notice_and_advances_once() {
        let (_temp, _home, store, group, _rule) = two_recipient_rule();
        let mut state = RuntimeState::default();
        state
            .last_rule
            .insert("reminder".into(), Utc::now().timestamp() - 120);
        let mut result = TickResult::default();
        let mut writes = 0;
        tick_rules_with(
            &store,
            &group,
            &mut state,
            &mut result,
            &mut |path, event| {
                writes += 1;
                if writes == 2 {
                    Err(io::Error::other("disk full"))
                } else {
                    ledger::append(path, event)
                }
            },
        )
        .expect("committed result");
        assert_eq!(result.notifications.len(), 1);
        assert!(result.rule_failure.is_some());
        assert_eq!(
            result.notifications[0].data["context"]["run_kind"],
            "scheduled"
        );
        assert_eq!(state.last_run["reminder"].kind, "scheduled");
        assert_eq!(state.fired_count["reminder"], 1);
        state::save(&store, &group.group_id, &state).expect("save state");
        assert_eq!(
            state::load(&store, &group.group_id)
                .expect("reload")
                .last_run["reminder"]
                .kind,
            "scheduled"
        );
    }

    #[test]
    fn state_write_failure_retains_notices_but_defers_control_actions() {
        let mut result = TickResult::default();
        result
            .notifications
            .push(Event::new("system.notify", "g_test"));
        result.actions.push(ScheduledAction::GroupState {
            group_id: "g_test".into(),
            state: "stopped".into(),
            rule_id: "stop".into(),
            fired_at: 1,
            one_time: true,
        });
        result
            .completed_rules
            .push(("g_test".into(), "limited".into()));
        result.rule_failure = Some(io::Error::other("notification append failed"));
        retain_committed_after_state_failure(&mut result, io::Error::other("state write failed"))
            .expect("retain committed notice");
        assert_eq!(result.notifications.len(), 1);
        assert!(result.actions.is_empty());
        assert!(result.completed_rules.is_empty());
        let error = result.rule_failure.expect("reported failure").to_string();
        assert!(error.contains("notification append failed"));
        assert!(error.contains("state write failed"));
    }

    /// Private replay evidence is supplied by the operator, never checked in.
    /// Run alone so the decode counter has no concurrent test producers.
    #[test]
    #[ignore = "requires CCCC_REAL_REPLAY_HOME pointing to copied real groups"]
    fn real_group_notices_match_history_without_warm_projection_decodes() {
        let root = std::env::var_os("CCCC_REAL_REPLAY_HOME").expect("copied-group home required");
        let home = HomeLayout::from_path(root).expect("replay home");
        let store = GroupStore::new(home.clone()).expect("group store");
        let groups = store.list().expect("real copied groups");
        assert!(!groups.is_empty(), "empty replay cannot prove a projection");
        for meta in groups {
            let group = store.load(&meta.group_id).expect("real group document");
            let eligible = actors::visible(&group)
                .filter(|actor| actor.enabled)
                .map(|actor| actor.id.clone())
                .collect::<HashSet<_>>();
            assert!(!eligible.is_empty(), "replay must keep enabled actors");
            // due_unread_notices differentially checks the original ordered
            // reducer's reply/read/delivery/withdrawal/claim decisions.
            due_unread_notices(
                &home,
                &store,
                &group,
                Some(&eligible),
                &HashSet::new(),
                UnreadPass::Scheduled,
            )
            .expect("cold projection/oracle");
            // The test-side legacy oracle intentionally reads history. This
            // counter measures only the production Snapshot decoder.
            let decoded = crate::ledger::snapshot::decoded_records();
            for _ in 0..5 {
                due_unread_notices(
                    &home,
                    &store,
                    &group,
                    Some(&eligible),
                    &HashSet::new(),
                    UnreadPass::Scheduled,
                )
                .expect("warm projection/oracle");
            }
            assert_eq!(
                crate::ledger::snapshot::decoded_records(),
                decoded,
                "warm unread projection decoded ledger history"
            );
        }
    }

    /// Each recipient of one Mail; notices are issued in this order.
    const RECIPIENTS: [&str; 2] = ["first", "second"];

    fn mail_for_both(home: &HomeLayout) -> (GroupStore, GroupDoc) {
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("notice handover", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                group.state = GroupState::Active;
                for id in RECIPIENTS {
                    actors::add(group, cccc_contracts::Actor::new(id))?;
                }
                group
                    .extra
                    .insert("delivery".into(), json!({"mail_notice_after_seconds":60}));
                Ok(())
            })
            .expect("group");
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        // Past every notice delay, so whichever pass runs announces it now.
        mail.ts = (Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
        mail.data = json!({"text":"review","to":RECIPIENTS,"message_mode":"mail"})
            .as_object()
            .cloned()
            .expect("mail");
        ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &mail).expect("Mail");
        let group = store.load(&group.group_id).expect("group");
        (store, group)
    }

    fn told(result: &TickResult) -> Vec<String> {
        result
            .notifications
            .iter()
            .filter(|event| event.data.get("kind") == Some(&json!("mail_notice")))
            .map(|event| {
                event.data["context"]["actor_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_failed_notice_write_still_hands_over_the_notices_written_before_it() {
        for pass in [UnreadPass::Scheduled, UnreadPass::IdleMailNow] {
            let temp = tempfile::tempdir().expect("tempdir");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            let (store, group) = mail_for_both(&home);
            let recipients = RECIPIENTS
                .map(str::to_owned)
                .into_iter()
                .collect::<HashSet<_>>();
            let pass_result = || {
                let mut result = TickResult::default();
                tick_unread(
                    &home,
                    &store,
                    &group,
                    Some(&recipients),
                    &recipients,
                    pass,
                    &mut result,
                )
                .expect("unread pass");
                result
            };

            // The second notice cannot be written: the first is already in the
            // ledger, so it must reach delivery from this pass.
            let (ledger_path, notices) =
                due_unread_notices(&home, &store, &group, Some(&recipients), &recipients, pass)
                    .expect("due notices")
                    .expect("notices to issue");
            let mut interrupted = TickResult::default();
            let mut writes = 0;
            issue_notices(
                &ledger_path,
                notices,
                &mut interrupted,
                &mut |path, event| {
                    writes += 1;
                    if writes == 2 {
                        Err(io::Error::other("disk full"))
                    } else {
                        ledger::append(path, event)
                    }
                },
            );
            assert!(interrupted.unread_failure.is_some(), "{pass:?}");
            assert_eq!(told(&interrupted), ["first"], "{pass:?}");

            // The next pass issues only the notice that was never written, and
            // after that nobody is told again.
            let next = pass_result();
            assert!(next.unread_failure.is_none(), "{pass:?}");
            assert_eq!(told(&next), ["second"], "{pass:?}");
            assert!(told(&pass_result()).is_empty(), "{pass:?}");
        }
    }

    #[test]
    fn a_failed_unread_discovery_preserves_an_already_issued_rule_notice() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let (store, group) = mail_for_both(&home);
        let recipients = RECIPIENTS
            .map(str::to_owned)
            .into_iter()
            .collect::<HashSet<_>>();
        let tick =
            || tick_group_with_idle_actors(&home, &group.group_id, true, &recipients, &recipients);
        let cursor_path = store
            .state_dir(&group.group_id)
            .expect("state directory")
            .join("read_cursors.json");
        std::fs::write(&cursor_path, b"invalid cursor JSON").expect("damaged cursor fixture");
        // Discovery failure before any rule result retains the normal Err path.
        assert!(tick().is_err());
        store
            .mutate(&group.group_id, |group| {
                group.automation = json!({"version":1,"rules":[{
                    "id":"one-shot","enabled":true,"to":[RECIPIENTS[0]],
                    "trigger":{"kind":"at","at":"2020-01-01T00:00:00Z"},
                    "action":{"kind":"notify","message":"scheduled reminder"}
                }]})
                .as_object()
                .cloned()
                .expect("rule");
                Ok(())
            })
            .expect("one-shot rule");

        let partial = tick().expect("issued rule must still reach delivery");
        assert!(partial.unread_failure.is_some());
        assert_eq!(partial.notifications.len(), 1);
        let issued = &partial.notifications[0];
        assert_eq!(issued.data["kind"], "automation");
        assert_eq!(issued.data["context"]["rule_id"], "one-shot");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let events = ledger::read_all(&ledger_path).expect("events");
        assert!(events.iter().any(|event| event.id == issued.id));
        assert!(
            state::load(&store, &group.group_id)
                .expect("saved rule state")
                .last_rule
                .contains_key("one-shot")
        );

        crate::fs::write_json(&cursor_path, &json!({"schema":1,"cursors":{}}))
            .expect("repaired cursor fixture");
        let next = tick().expect("next pass");
        assert!(next.unread_failure.is_none());
        assert_eq!(told(&next), RECIPIENTS);
        assert!(
            next.notifications
                .iter()
                .all(|event| event.data["kind"] != "automation")
        );
        let events = ledger::read_all(&ledger_path).expect("events after repair");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.data.get("kind") == Some(&json!("automation")))
                .count(),
            1
        );
        assert!(tick().expect("settled pass").notifications.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_rule_state_save_issues_no_notice_it_cannot_hand_over() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let (store, group) = mail_for_both(&home);
        // A rule seen for the first time changes the saved rule state.
        store
            .mutate(&group.group_id, |group| {
                group.automation = json!({"version":1,"rules":[{
                    "id":"reminder","enabled":true,"to":["@all"],
                    "trigger":{"kind":"interval","every_seconds":3600},
                    "action":{"kind":"notify","message":"check in"}
                }]})
                .as_object()
                .cloned()
                .expect("rules");
                Ok(())
            })
            .expect("rules");
        let recipients = RECIPIENTS
            .map(str::to_owned)
            .into_iter()
            .collect::<HashSet<_>>();
        let tick =
            || tick_group_with_idle_actors(&home, &group.group_id, true, &recipients, &recipients);
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let written_notices = || {
            ledger::inspect(&ledger_path, |events, _| {
                events
                    .iter()
                    .filter(|event| event.data.get("kind") == Some(&json!("mail_notice")))
                    .count()
            })
            .expect("ledger")
        };
        // The ledger lives under the state directory and stays writable; only
        // the rule state file next to it cannot be created.
        let state_dir = store.state_dir(&group.group_id).expect("state dir");
        let set_mode = |mode| {
            std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(mode))
                .expect("permissions");
        };

        set_mode(0o555);
        let failed = tick();
        set_mode(0o755);
        assert!(failed.is_err());
        assert_eq!(written_notices(), 0, "a notice nobody receives was issued");

        let retried = tick().expect("retry");
        assert_eq!(told(&retried), RECIPIENTS);
    }

    #[test]
    fn unread_tick_without_eligible_recipients_does_not_load_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("idle cost", "").expect("group");
        group.state = GroupState::Active;
        let path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&path, &Event::new("chat.message", &group.group_id)).expect("append");
        for mode in ["no actors", "disabled", "not running"] {
            if mode != "no actors" {
                let mut actor = cccc_contracts::Actor::new("peer");
                actor.enabled = mode != "disabled";
                group.actors = vec![actor];
            }
            store.save(&group).expect("save");
            crate::ledger_index::invalidate_path(&path);
            let result = if mode == "not running" {
                tick_group_for_delivery_actors(&home, &group.group_id, true, &HashSet::new())
            } else {
                tick_group(&home, &group.group_id, true)
            }
            .expect("tick");
            assert!(result.notifications.is_empty());
            assert!(!crate::ledger_index::is_cached(&path), "{mode}");
        }
    }
}

#[cfg(test)]
#[path = "automation/mail_notice_repeat_tests.rs"]
mod mail_notice_repeat_tests;

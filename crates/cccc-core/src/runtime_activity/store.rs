use super::{EVENT_LIMIT, RETENTION_SECONDS, RuntimeActivityEvent};
use crate::codex_hook_state::CodexHookState;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::io;
use uuid::Uuid;

pub(super) fn prune_events(
    events: &mut Vec<RuntimeActivityEvent>,
    now: DateTime<Utc>,
) -> io::Result<()> {
    events.retain(|event| {
        DateTime::parse_from_rfc3339(&event.ts)
            .map(|timestamp| {
                (now - timestamp.with_timezone(&Utc)).num_seconds() <= RETENTION_SECONDS
            })
            .unwrap_or(false)
    });
    enforce_event_limit(events)
}

pub(super) fn terminalize_active_activities(
    events: &mut Vec<RuntimeActivityEvent>,
    state: &CodexHookState,
    event_type: &str,
    status: &str,
    now: &str,
) -> usize {
    let mut latest = BTreeMap::<(String, String), RuntimeActivityEvent>::new();
    for event in events.iter().filter(|event| {
        event.actor_id == state.actor_id
            && event.session_id == state.session_id
            && event.kind != "session"
            && is_active(event)
    }) {
        latest.insert(
            (event.actor_id.clone(), event.activity_id.clone()),
            event.clone(),
        );
    }
    let active = latest.into_values().collect::<Vec<_>>();
    for started in &active {
        let started_at = events
            .iter()
            .find(|event| {
                event.actor_id == started.actor_id
                    && event.activity_id == started.activity_id
                    && event.status == "started"
            })
            .map(|event| event.ts.as_str())
            .unwrap_or(started.ts.as_str());
        let start = DateTime::parse_from_rfc3339(started_at).ok();
        let end = DateTime::parse_from_rfc3339(now).ok();
        let duration_ms = start
            .zip(end)
            .and_then(|(start, end)| u64::try_from((end - start).num_milliseconds().max(0)).ok());
        events.retain(|event| {
            event.actor_id != started.actor_id || event.activity_id != started.activity_id
        });
        events.push(RuntimeActivityEvent {
            id: Uuid::new_v4().simple().to_string(),
            ts: now.to_owned(),
            status: status.to_owned(),
            event_type: event_type.to_owned(),
            duration_ms,
            ..started.clone()
        });
    }
    active.len()
}

pub(super) fn enforce_event_limit(events: &mut Vec<RuntimeActivityEvent>) -> io::Result<()> {
    while events.len() > EVENT_LIMIT {
        let Some(index) = events
            .iter()
            .position(|event| !is_active(event))
            .or_else(|| {
                events
                    .iter()
                    .position(|event| matches!(event.status.as_str(), "waiting" | "stuck"))
            })
        else {
            return Err(io::Error::other(
                "runtime activity capacity exhausted by active events",
            ));
        };
        events.remove(index);
    }
    Ok(())
}

fn is_active(event: &RuntimeActivityEvent) -> bool {
    matches!(event.status.as_str(), "started" | "waiting" | "stuck")
}

use crate::codex_hook_state::CodexHookState;
use crate::fs::{read_json, with_exclusive_lock, write_json_committed as write_json};
use crate::{GroupStore, HomeLayout};
use cccc_contracts::utc_now;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod project;
#[cfg(test)]
use project::sanitize_label;
use project::{ActivityDraft, project_hook_event};
mod store;
use store::{enforce_event_limit, prune_events, terminalize_active_activities};

const VERSION: u8 = 1;
const EVENT_LIMIT: usize = 256;
const RETENTION_SECONDS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeActivityEvent {
    pub v: u8,
    pub id: String,
    pub ts: String,
    pub group_id: String,
    pub actor_id: String,
    pub runtime: String,
    pub activity_id: String,
    pub kind: String,
    pub status: String,
    pub event_type: String,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

pub fn record_hook_event(
    home: &HomeLayout,
    runtime: &str,
    launch_token: &str,
    payload: &Value,
    state: &CodexHookState,
) -> io::Result<Option<RuntimeActivityEvent>> {
    let Some(draft) = project_hook_event(runtime, launch_token, payload, state) else {
        return Ok(None);
    };
    let events_path = events_path(home, &state.group_id)?;
    with_exclusive_lock(&lock_path(home, &state.group_id)?, || {
        let mut events = read_event_file(&events_path)?;
        prune_events(&mut events, Utc::now())?;
        let mut terminalized = 0;
        if draft.kind == "session" && draft.status == "completed" {
            terminalized += terminalize_active_activities(
                &mut events,
                state,
                "SessionEnded",
                "failed",
                &utc_now(),
            );
        } else if draft.kind == "turn" && draft.status == "started" {
            terminalized += terminalize_active_activities(
                &mut events,
                state,
                "TurnSuperseded",
                "failed",
                &utc_now(),
            );
        }
        if runtime == "claude" && draft.kind == "session" {
            if terminalized > 0 {
                enforce_event_limit(&mut events)?;
                write_json(&events_path, &events)?;
            }
            return Ok(None);
        }
        if events.iter().rev().any(|event| {
            event.actor_id == state.actor_id
                && event.activity_id == draft.activity_id
                && event.event_type == draft.event_type
                && event.status == draft.status
        }) {
            return Ok(None);
        }
        let now = utc_now();
        let duration_ms = completion_duration_ms(&events, state, &draft, &now);
        let tool_name = draft.tool_name.clone().or_else(|| {
            events
                .iter()
                .rev()
                .find(|event| {
                    event.actor_id == state.actor_id
                        && event.activity_id == draft.activity_id
                        && event.tool_name.is_some()
                })
                .and_then(|event| event.tool_name.clone())
        });
        events.retain(|event| {
            if event.actor_id != state.actor_id || event.activity_id != draft.activity_id {
                return true;
            }
            draft.status == "waiting" && event.status == "started"
        });
        let event = RuntimeActivityEvent {
            v: VERSION,
            id: Uuid::new_v4().simple().to_string(),
            ts: now,
            group_id: state.group_id.clone(),
            actor_id: state.actor_id.clone(),
            runtime: runtime.to_owned(),
            activity_id: draft.activity_id,
            kind: draft.kind.to_owned(),
            status: draft.status.to_owned(),
            event_type: draft.event_type,
            session_id: state.session_id.clone(),
            turn_id: draft.turn_id,
            operation_id: draft.operation_id,
            tool_name,
            duration_ms,
        };
        events.push(event.clone());
        enforce_event_limit(&mut events)?;
        write_json(&events_path, &events)?;
        Ok(Some(event))
    })
}

pub fn close_actor_activities(
    home: &HomeLayout,
    state: &CodexHookState,
    event_type: &str,
) -> io::Result<()> {
    let events_path = events_path(home, &state.group_id)?;
    with_exclusive_lock(&lock_path(home, &state.group_id)?, || {
        let mut events = read_event_file(&events_path)?;
        prune_events(&mut events, Utc::now())?;
        if terminalize_active_activities(&mut events, state, event_type, "failed", &utc_now()) == 0
        {
            return Ok(());
        }
        enforce_event_limit(&mut events)?;
        write_json(&events_path, &events)
    })
}

pub fn read_events(home: &HomeLayout, group_id: &str) -> io::Result<Vec<RuntimeActivityEvent>> {
    let path = events_path(home, group_id)?;
    let mut events = read_event_file(&path)?;
    prune_events(&mut events, Utc::now())?;
    Ok(events)
}

fn completion_duration_ms(
    events: &[RuntimeActivityEvent],
    state: &CodexHookState,
    draft: &ActivityDraft,
    completed_at: &str,
) -> Option<u64> {
    if !matches!(draft.status, "completed" | "failed") {
        return None;
    }
    let started = events.iter().rev().find(|event| {
        event.actor_id == state.actor_id
            && event.activity_id == draft.activity_id
            && event.status == "started"
    })?;
    let start = DateTime::parse_from_rfc3339(&started.ts).ok()?;
    let end = DateTime::parse_from_rfc3339(completed_at).ok()?;
    u64::try_from((end - start).num_milliseconds().max(0)).ok()
}

fn read_event_file(path: &Path) -> io::Result<Vec<RuntimeActivityEvent>> {
    match read_json(path) {
        Ok(events) => Ok(events),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn events_path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("runtime-activity/events.json"))
}

fn lock_path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("runtime-activity/events.lock"))
}

#[cfg(test)]
#[path = "runtime_activity_tests.rs"]
mod tests;

//! Provider-owned turn state for a single PTY generation. No output inference.
mod hermes;
mod process;

use cccc_contracts::{Actor, ActorRuntime, TurnProbe};
use cccc_runtime::SessionStatus;
use process::{ProcFs, Process, Processes};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TurnObservation {
    Working { since: Option<String> },
    Idle { since: Option<String> },
    Unknown { why: &'static str },
}

#[derive(Clone)]
struct Entry {
    started_at: String,
    root: Process,
    hermes: hermes::Hermes,
    observation: TurnObservation,
}

type Key = (String, String);
fn entries() -> &'static Mutex<BTreeMap<Key, Entry>> {
    static ENTRIES: OnceLock<Mutex<BTreeMap<Key, Entry>>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}

/// Called once at a successful launch: saved configuration changes wait for restart.
pub(super) fn started(actor: &Actor, status: &SessionStatus) {
    let key = (status.group_id.clone(), status.actor_id.clone());
    let Ok(mut entries) = entries().lock() else {
        return;
    };
    entries.remove(&key);
    if actor.runtime != ActorRuntime::Hermes || actor.turn_probe == TurnProbe::Off {
        return;
    }
    let Some(pid) = status.pid.filter(|_| status.running) else {
        return;
    };
    let Ok(root) = ProcFs.read(pid) else {
        return;
    };
    entries.insert(
        key,
        Entry {
            started_at: status.started_at.clone(),
            root,
            hermes: hermes::Hermes::default(),
            observation: TurnObservation::Unknown {
                why: "not_observed",
            },
        },
    );
}

/// Runs only on the ordinary activity publisher tick, never a mutating request.
pub(super) fn refresh(group: &str, actor: &Actor) {
    let key = (group.to_owned(), actor.id.clone());
    let entry = entries()
        .lock()
        .ok()
        .and_then(|entries| entries.get(&key).cloned());
    let Some(mut entry) = entry else {
        return;
    };
    let Some(status) = generation(group, &actor.id, &entry) else {
        if let Ok(mut entries) = entries().lock() {
            entries.remove(&key);
        }
        return;
    };
    // Registered managed authority always wins, including late registration.
    entry.observation = if !super::local_headless::has_no_registered_session(group, &actor.id)
        || super::local_headless::status(group, &actor.id).is_some()
    {
        TurnObservation::Unknown {
            why: "managed_session",
        }
    } else {
        observe(&mut entry, group, &actor.id)
    };
    if let Ok(mut entries) = entries().lock()
        && let Some(current) = entries.get_mut(&key)
        && current.started_at == status.started_at
        && current.root == entry.root
    {
        *current = entry;
    }
}

fn observe(entry: &mut Entry, group: &str, actor: &str) -> TurnObservation {
    let result = (|| {
        let home = process::provider_home(entry.root.pid, group, actor)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock_unavailable")?
            .as_secs_f64();
        Ok(entry
            .hermes
            .observe(&home.join("state.db"), &ProcFs, &entry.root, now))
    })();
    result.unwrap_or_else(|why| TurnObservation::Unknown { why })
}

fn generation(group: &str, actor: &str, entry: &Entry) -> Option<SessionStatus> {
    let status = cccc_runtime::status(group, actor).ok()?;
    if !status.running
        || status.pid != Some(entry.root.pid)
        || status.started_at != entry.started_at
    {
        return None;
    }
    let root = ProcFs.read(entry.root.pid).ok()?;
    (root.start == entry.root.start && root.namespace == entry.root.namespace).then_some(status)
}

/// Cached state only. A new/reused PTY identity cannot inherit an observation.
pub(super) fn cached(group: &str, actor: &str) -> Option<TurnObservation> {
    let entry = entries()
        .lock()
        .ok()?
        .get(&(group.into(), actor.into()))
        .cloned()?;
    if !super::local_headless::has_no_registered_session(group, actor) {
        return None;
    }
    generation(group, actor, &entry)?;
    match entry.observation {
        TurnObservation::Unknown { .. } => None,
        observation => Some(observation),
    }
}

pub(super) fn fields(
    observation: TurnObservation,
) -> (String, String, Option<String>, Option<String>) {
    let (state, since) = match observation {
        TurnObservation::Working { since } => ("working", since),
        TurnObservation::Idle { since } => ("idle", since),
        TurnObservation::Unknown { .. } => {
            unreachable!("Unknown falls through before field construction")
        }
    };
    // A provider conversation/turn id is not a CCCC task id.
    (
        state.into(),
        "turn_probe:hermes_turn_lease".into(),
        since,
        None,
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests;

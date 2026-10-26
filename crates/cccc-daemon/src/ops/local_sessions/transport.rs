//! Local transport handoff. Existing delivery workers and the ledger retain ownership.
use super::store;
use cccc_core::HomeLayout;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

const HANDOFF_TIMEOUT: Duration = Duration::from_secs(30);
const QUEUE_CAPACITY: usize = 256;
type Key = (PathBuf, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Accepted,
    Rejected(String),
    Ambiguous(String),
    Unavailable,
}

#[derive(Clone, Serialize)]
pub(super) struct Handoff {
    pub handoff_id: String,
    pub source_id: String,
    pub native_session_id: String,
    pub group_id: String,
    pub actor_id: String,
    #[serde(skip)]
    pub actor_generation: String,
    pub event_ids: Vec<String>,
    pub text: String,
}

struct Pending {
    handoff: Handoff,
    connection_id: String,
    revision: String,
    result: mpsc::Sender<Outcome>,
    offered: bool,
}

#[derive(Default)]
struct Queue {
    ready: VecDeque<String>,
    pending: HashMap<String, Pending>,
}

fn queues() -> &'static Mutex<HashMap<Key, Queue>> {
    static QUEUES: OnceLock<Mutex<HashMap<Key, Queue>>> = OnceLock::new();
    QUEUES.get_or_init(Mutex::default)
}

pub(super) fn submit(home: &HomeLayout, mut handoff: Handoff, cancelled: &AtomicBool) -> Outcome {
    let Some(binding) = store::by_source(home, &handoff.source_id).ok().flatten() else {
        return Outcome::Unavailable;
    };
    handoff.native_session_id = binding.native_session_id;
    handoff.handoff_id = uuid::Uuid::new_v4().to_string();
    let key = (home.root().to_owned(), handoff.source_id.clone());
    let id = handoff.handoff_id.clone();
    let (sender, result) = mpsc::channel();
    {
        let Ok(mut queues) = queues().lock() else {
            return Outcome::Unavailable;
        };
        let queue = queues.entry(key.clone()).or_default();
        if queue.pending.len() >= QUEUE_CAPACITY {
            return Outcome::Unavailable;
        }
        queue.ready.push_back(id.clone());
        queue.pending.insert(
            id.clone(),
            Pending {
                handoff,
                connection_id: binding.connection_id,
                revision: binding.revision,
                result: sender,
                offered: false,
            },
        );
    }
    let deadline = Instant::now() + HANDOFF_TIMEOUT;
    let outcome = loop {
        match result.recv_timeout(Duration::from_millis(100)) {
            Ok(outcome) => break Some(outcome),
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            break None;
        }
    };
    // Withdraw the pending registration and its offered state under one lock:
    // `next` must either offer it first or find no work, never offer after we
    // classify an unsubmitted attempt as safe to retry.
    let pending = if let Ok(mut queues) = queues().lock() {
        let mut pending = None;
        if let Some(queue) = queues.get_mut(&key) {
            pending = queue.pending.remove(&id);
            queue.ready.retain(|candidate| candidate != &id);
            if queue.pending.is_empty() {
                queues.remove(&key);
            }
        }
        pending
    } else {
        None
    };
    // An ACK/detach may have removed the item and sent a result after the last
    // recv timeout. Preserve that already settled outcome, especially Accepted.
    outcome
        .or_else(|| result.try_recv().ok())
        .unwrap_or_else(|| match pending {
            Some(pending) if !pending.offered => Outcome::Unavailable,
            _ => Outcome::Ambiguous("Native receiver did not confirm this handoff".into()),
        })
}

pub(super) fn next(
    home: &HomeLayout,
    receiver: &store::Receiver,
) -> Result<Option<Handoff>, crate::dispatch::OpError> {
    let key = (home.root().to_owned(), receiver.source_id());
    let mut queues = queues().lock().map_err(|_| unavailable())?;
    let Some(queue) = queues.get_mut(&key) else {
        return Ok(None);
    };
    // Sweep offered items too: they have left `ready`, but an old receiver's
    // missing ACK must not hold the worker after /clear or receiver replacement.
    queue.pending.retain(|_, pending| {
        let current = pending.connection_id == receiver.connection_id
            && pending.revision == receiver.revision
            && target_enabled(home, &pending.handoff);
        if !current {
            settle_unavailable(pending);
        }
        current
    });
    queue.ready.retain(|id| queue.pending.contains_key(id));
    while let Some(id) = queue.ready.pop_front() {
        let Some(pending) = queue.pending.get_mut(&id) else {
            continue;
        };
        if pending.connection_id == receiver.connection_id && pending.revision == receiver.revision
        {
            pending.offered = true;
            return Ok(Some(pending.handoff.clone()));
        }
        if let Some(pending) = queue.pending.remove(&id) {
            let outcome = if pending.offered {
                Outcome::Ambiguous(
                    "Native receiver changed before acknowledging the handoff".into(),
                )
            } else {
                Outcome::Unavailable
            };
            let _ = pending.result.send(outcome);
        }
    }
    Ok(None)
}

pub(super) fn acknowledge(
    home: &HomeLayout,
    receiver: &store::Receiver,
    id: &str,
    outcome: Outcome,
) -> Result<(), crate::dispatch::OpError> {
    let mut queues = queues().lock().map_err(|_| unavailable())?;
    let key = (home.root().to_owned(), receiver.source_id());
    let queue = queues.get_mut(&key).ok_or_else(stale)?;
    let pending = queue.pending.get(id).ok_or_else(stale)?;
    if !pending.offered
        || pending.connection_id != receiver.connection_id
        || pending.revision != receiver.revision
    {
        return Err(stale());
    }
    let pending = queue.pending.remove(id).expect("validated pending handoff");
    let _ = pending.result.send(outcome);
    Ok(())
}

fn target_enabled(home: &HomeLayout, handoff: &Handoff) -> bool {
    cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(&handoff.group_id))
        .is_ok_and(|group| {
            !matches!(
                group.state,
                cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
            ) && group.actors.iter().any(|actor| {
                actor.id == handoff.actor_id
                    && cccc_core::actors::generation_identity(actor) == handoff.actor_generation
                    && actor.enabled
                    && actor.external_session_id == handoff.source_id
            })
        })
}

fn settle_unavailable(pending: &Pending) {
    let outcome = if pending.offered {
        Outcome::Ambiguous("Native receiver changed before acknowledging the handoff".into())
    } else {
        Outcome::Unavailable
    };
    let _ = pending.result.send(outcome);
}

pub(super) fn detach(
    home: &HomeLayout,
    source_id: &str,
    connection_id: &str,
) -> Result<(), crate::dispatch::OpError> {
    let key = (home.root().to_owned(), source_id.into());
    let mut queues = queues().lock().map_err(|_| unavailable())?;
    if let Some(queue) = queues.get_mut(&key) {
        queue.pending.retain(|_, pending| {
            if pending.connection_id != connection_id {
                return true;
            }
            settle_unavailable(pending);
            false
        });
        queue.ready.retain(|id| queue.pending.contains_key(id));
        if queue.pending.is_empty() {
            queues.remove(&key);
        }
    }
    Ok(())
}

fn unavailable() -> crate::dispatch::OpError {
    crate::dispatch::OpError::new(
        "local_session_unavailable",
        "Receiver transport is unavailable",
    )
}

fn stale() -> crate::dispatch::OpError {
    crate::dispatch::OpError::new(
        "local_session_stale_handoff",
        "This handoff is no longer owned by the current receiver",
    )
}

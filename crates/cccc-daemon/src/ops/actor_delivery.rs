use cccc_contracts::{Actor, ActorRuntime, Event, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};

use crate::ops::actor_delivery_worker;

const QUEUE_CAPACITY: usize = 256;
const COMPLETION_CAPACITY: usize = 4096;

type Key = (String, String);

#[derive(Debug, Clone, Serialize)]
pub struct DispatchReport {
    pub accepted: bool,
    pub state: &'static str,
    pub targeted: usize,
    pub online: usize,
    pub queued: usize,
}

#[derive(Clone)]
pub(super) struct DeliveryJob {
    pub home: HomeLayout,
    pub group: GroupDoc,
    pub actor: Actor,
    pub event: Event,
}

pub(super) struct DeliveryCompletion {
    pub group_id: String,
    pub actor_id: String,
    pub event_id: String,
}

struct DeliveryWorker {
    sender: Option<SyncSender<DeliveryJob>>,
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DeliveryWorker {
    fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.sender.take();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("PTY delivery worker panicked during shutdown");
        }
    }
}

impl Drop for DeliveryWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn workers() -> &'static Mutex<HashMap<Key, DeliveryWorker>> {
    static WORKERS: OnceLock<Mutex<HashMap<Key, DeliveryWorker>>> = OnceLock::new();
    WORKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn completions() -> &'static Mutex<VecDeque<DeliveryCompletion>> {
    static COMPLETIONS: OnceLock<Mutex<VecDeque<DeliveryCompletion>>> = OnceLock::new();
    COMPLETIONS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn in_flight() -> &'static Mutex<HashSet<(String, String, String)>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<(String, String, String)>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn record_completion(completion: DeliveryCompletion) {
    if let Ok(mut queue) = completions().lock() {
        if queue.len() >= COMPLETION_CAPACITY {
            if let Some(dropped) = queue.pop_front() {
                clear_in_flight(|item| {
                    item.0 == dropped.group_id
                        && item.1 == dropped.actor_id
                        && item.2 == dropped.event_id
                });
            }
            tracing::warn!("PTY delivery completion queue reached capacity");
        }
        queue.push_back(completion);
    }
}

pub fn shutdown_actor(group_id: &str, actor_id: &str) {
    let worker = workers()
        .lock()
        .ok()
        .and_then(|mut workers| workers.remove(&(group_id.to_owned(), actor_id.to_owned())));
    if let Some(worker) = worker {
        worker.shutdown();
    }
    remove_completions(|item| item.group_id == group_id && item.actor_id == actor_id);
    clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
}

pub fn shutdown_group(group_id: &str) {
    let removed = workers()
        .lock()
        .map(|mut workers| {
            let keys = workers
                .keys()
                .filter(|(worker_group_id, _)| worker_group_id == group_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| workers.remove(&key))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    remove_completions(|item| item.group_id == group_id);
    clear_in_flight(|item| item.0 == group_id);
}

pub fn shutdown_all() {
    let removed = workers()
        .lock()
        .map(|mut workers| {
            std::mem::take(&mut *workers)
                .into_values()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for worker in removed {
        worker.shutdown();
    }
    if let Ok(mut completions) = completions().lock() {
        completions.clear();
    }
    if let Ok(mut pending) = in_flight().lock() {
        pending.clear();
    }
}

fn remove_completions(mut remove: impl FnMut(&DeliveryCompletion) -> bool) {
    if let Ok(mut completions) = completions().lock() {
        completions.retain(|item| !remove(item));
    }
}

fn clear_in_flight(mut remove: impl FnMut(&(String, String, String)) -> bool) {
    if let Ok(mut pending) = in_flight().lock() {
        pending.retain(|item| !remove(item));
    }
}

pub(super) fn release_in_flight(job: &DeliveryJob) {
    if let Ok(mut pending) = in_flight().lock() {
        pending.remove(&(
            job.group.group_id.clone(),
            job.actor.id.clone(),
            job.event.id.clone(),
        ));
    }
}

pub fn dispatch(home: &HomeLayout, group: &GroupDoc, event: &Event) -> DispatchReport {
    if !matches!(event.kind.as_str(), "chat.message" | "system.notify")
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
    {
        return report(0, 0, 0);
    }

    let targets: Vec<_> = group
        .actors
        .iter()
        .filter(|actor| {
            actor.runtime != ActorRuntime::WebModel && inbox::is_for_actor(group, event, &actor.id)
        })
        .cloned()
        .collect();
    let mut queued = 0;
    let mut online = 0;
    for actor in &targets {
        if cccc_runtime::status(&group.group_id, &actor.id).is_ok_and(|status| status.running) {
            online += 1;
            if enqueue(DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event: event.clone(),
            }) {
                queued += 1;
            }
        }
    }
    report(targets.len(), online, queued)
}

pub fn replay_unread(home: &HomeLayout, group: &GroupDoc, actor_id: &str) -> usize {
    let Some(actor) = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id && actor.runtime != ActorRuntime::WebModel)
        .cloned()
    else {
        return 0;
    };
    if !cccc_runtime::status(&group.group_id, actor_id).is_ok_and(|status| status.running) {
        return 0;
    }
    inbox::list_unread(home, group, actor_id, 1000)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| actor.created_at.is_empty() || event.ts >= actor.created_at)
        .filter(|event| {
            enqueue(DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event: event.clone(),
            })
        })
        .count()
}

pub fn drain(home: &HomeLayout) {
    let pending = completions()
        .lock()
        .map(|mut queue| queue.drain(..).collect::<Vec<_>>())
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let mut grouped = HashMap::<Key, Vec<DeliveryCompletion>>::new();
    for completion in pending {
        grouped
            .entry((completion.group_id.clone(), completion.actor_id.clone()))
            .or_default()
            .push(completion);
    }
    let mut deferred = VecDeque::new();
    let mut refill = HashSet::new();
    for ((group_id, actor_id), batch) in grouped {
        let Ok(group) = store.load(&group_id) else {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        };
        if !auto_mark_on_delivery(&group) {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        }
        let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
            clear_in_flight(|item| item.0 == group_id && item.1 == actor_id);
            continue;
        };
        let unread = inbox::list_unread(home, &group, &actor_id, 1000)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| actor.created_at.is_empty() || event.ts >= actor.created_at)
            .collect::<Vec<_>>();
        let unread_ids = unread
            .iter()
            .map(|event| event.id.clone())
            .collect::<HashSet<_>>();
        let completed_ids = batch
            .iter()
            .map(|completion| completion.event_id.clone())
            .collect::<HashSet<_>>();
        let delivered = unread
            .iter()
            .take_while(|event| completed_ids.contains(&event.id))
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        let delivered_ids = delivered.iter().cloned().collect::<HashSet<_>>();
        let resolved_ids = batch
            .iter()
            .filter(|completion| {
                delivered_ids.contains(&completion.event_id)
                    || !unread_ids.contains(&completion.event_id)
            })
            .map(|completion| completion.event_id.clone())
            .collect::<HashSet<_>>();
        for completion in batch {
            if !resolved_ids.contains(&completion.event_id) {
                deferred.push_back(completion);
            }
        }
        clear_in_flight(|item| {
            item.0 == group_id && item.1 == actor_id && resolved_ids.contains(&item.2)
        });
        if !resolved_ids.is_empty() {
            refill.insert((group_id.clone(), actor_id.clone()));
        }
        let advanced = delivered.last().is_some_and(|event_id| {
            inbox::advance(home, &group_id, &actor_id, event_id).unwrap_or(false)
        });
        if advanced {
            let event_id = delivered.last().cloned().unwrap_or_default();
            let mut event = Event::new("chat.read", &group_id);
            event.by.clone_from(&actor_id);
            event.data = json!({
                "actor_id": actor_id,
                "event_id": event_id,
                "delivered_count": delivered.len(),
                "source": "runtime_delivery",
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            if let Ok(path) = store.ledger_path(&event.group_id) {
                let _ = ledger::append(&path, &event);
            }
        }
    }
    if let Ok(mut completions) = completions().lock() {
        completions.extend(deferred);
    }
    for (group_id, actor_id) in refill {
        if let Ok(group) = store.load(&group_id) {
            replay_unread(home, &group, &actor_id);
        }
    }
}

fn report(targeted: usize, online: usize, queued: usize) -> DispatchReport {
    let state = if queued > 0 {
        "queued"
    } else if online > 0 {
        "queue_full"
    } else if targeted > 0 {
        "inbox"
    } else {
        "no_recipients"
    };
    DispatchReport {
        accepted: true,
        state,
        targeted,
        online,
        queued,
    }
}

fn enqueue(job: DeliveryJob) -> bool {
    let key = (job.group.group_id.clone(), job.actor.id.clone());
    let delivery_key = (key.0.clone(), key.1.clone(), job.event.id.clone());
    let reserved = in_flight()
        .lock()
        .map(|mut pending| pending.insert(delivery_key.clone()))
        .unwrap_or(false);
    if !reserved {
        return false;
    }
    let mut map = match workers().lock() {
        Ok(map) => map,
        Err(_) => {
            clear_in_flight(|item| item == &delivery_key);
            return false;
        }
    };
    let worker = map.entry(key.clone()).or_insert_with(|| spawn_worker(&key));
    let Some(sender) = worker.sender.as_ref() else {
        clear_in_flight(|item| item == &delivery_key);
        return false;
    };
    match sender.try_send(job) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            clear_in_flight(|item| item == &delivery_key);
            tracing::warn!(group_id = %key.0, actor_id = %key.1, "PTY delivery queue is full");
            false
        }
        Err(TrySendError::Disconnected(job)) => {
            let worker = spawn_worker(&key);
            let result = worker
                .sender
                .as_ref()
                .is_some_and(|sender| sender.try_send(job).is_ok());
            let stale = map.insert(key, worker);
            drop(map);
            if let Some(stale) = stale {
                stale.shutdown();
            }
            if !result {
                clear_in_flight(|item| item == &delivery_key);
            }
            result
        }
    }
}

fn spawn_worker(key: &Key) -> DeliveryWorker {
    let (sender, receiver) = mpsc::sync_channel::<DeliveryJob>(QUEUE_CAPACITY);
    let name = format!("cccc-delivery:{}:{}", key.0, key.1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    let thread = std::thread::Builder::new().name(name).spawn(move || {
        let mut preamble_session = String::new();
        let mut last_delivery = None;
        while !thread_cancelled.load(Ordering::Acquire) {
            let Ok(job) = receiver.recv() else {
                break;
            };
            let mut delivered = false;
            for attempt in 0..3 {
                if actor_delivery_worker::process(
                    &job,
                    &mut preamble_session,
                    &mut last_delivery,
                    &thread_cancelled,
                ) {
                    delivered = true;
                    break;
                }
                if thread_cancelled.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250 * (attempt + 1)));
            }
            if !delivered {
                release_in_flight(&job);
            }
            replay_unread(&job.home, &job.group, &job.actor.id);
        }
    });
    let thread = match thread {
        Ok(thread) => Some(thread),
        Err(error) => {
            tracing::warn!(%error, "failed to start actor delivery worker");
            None
        }
    };
    DeliveryWorker {
        sender: thread.as_ref().map(|_| sender),
        cancelled,
        thread,
    }
}

fn auto_mark_on_delivery(group: &GroupDoc) -> bool {
    group
        .extra
        .get("settings")
        .and_then(|value| value.get("auto_mark_on_delivery"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn actor_and_group_shutdown_remove_workers() {
        let first = ("g_cleanup".to_owned(), "actor-1".to_owned());
        let second = ("g_cleanup".to_owned(), "actor-2".to_owned());
        workers().lock().expect("workers").extend([
            (first.clone(), spawn_worker(&first)),
            (second.clone(), spawn_worker(&second)),
        ]);

        shutdown_actor(&first.0, &first.1);
        assert!(!workers().lock().expect("workers").contains_key(&first));
        shutdown_group(&second.0);
        assert!(!workers().lock().expect("workers").contains_key(&second));
    }
}

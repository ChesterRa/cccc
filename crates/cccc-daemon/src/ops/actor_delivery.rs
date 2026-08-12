use cccc_contracts::{Actor, ActorRuntime, Event, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};

use crate::ops::actor_delivery_worker;

mod drain;
mod lifecycle;
pub(crate) use drain::{drain_group, pending_group_ids};
pub use lifecycle::{shutdown_actor, shutdown_all, shutdown_group};

const QUEUE_CAPACITY: usize = 256;
const COMPLETION_CAPACITY: usize = 4096;
const BATCH_CAPACITY: usize = 64;
const BATCH_WINDOW: std::time::Duration = std::time::Duration::from_millis(250);
const DEFERRED_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(4);

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
            (!crate::ops::actor_runtime::is_structured(actor)
                || crate::ops::local_headless::supports(actor))
                && inbox::is_for_actor(group, event, &actor.id)
        })
        .cloned()
        .collect();
    let mut queued = 0;
    let mut online = 0;
    for actor in &targets {
        let actor_online = if crate::ops::local_headless::supports(actor) {
            crate::ops::local_headless::running(&group.group_id, &actor.id)
        } else {
            cccc_runtime::status(&group.group_id, &actor.id).is_ok_and(|status| status.running)
        };
        if actor_online {
            online += 1;
        }
        if enqueue(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
        }) {
            queued += 1;
        }
    }
    report(targets.len(), online, queued)
}

pub fn dispatch_unread(home: &HomeLayout, group: &GroupDoc, actor_id: &str) -> usize {
    if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return 0;
    }
    let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
        return 0;
    };
    if !actor.enabled
        || (crate::ops::actor_runtime::is_structured(actor)
            && !crate::ops::local_headless::supports(actor))
    {
        return 0;
    }
    let events = match inbox::list_unread(home, group, actor_id, QUEUE_CAPACITY, "all") {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(
                %error,
                group_id = %group.group_id,
                %actor_id,
                "failed to reload unread runtime deliveries"
            );
            return 0;
        }
    };
    events
        .into_iter()
        .filter(|event| matches!(event.kind.as_str(), "chat.message" | "system.notify"))
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

pub fn dispatch_group_unread(home: &HomeLayout, group: &GroupDoc) -> usize {
    group
        .actors
        .iter()
        .map(|actor| dispatch_unread(home, group, &actor.id))
        .sum()
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

pub(super) fn delivery_setting<'a>(
    group: &'a GroupDoc,
    key: &str,
) -> Option<&'a serde_json::Value> {
    group
        .extra
        .get("delivery")
        .and_then(|value| value.get(key))
        .or_else(|| group.extra.get("settings").and_then(|value| value.get(key)))
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
        let mut deferred = Vec::new();
        let mut deferred_failures: u32 = 0;
        while !thread_cancelled.load(Ordering::Acquire) {
            let mut batch = if deferred.is_empty() {
                let Ok(job) = receiver.recv() else {
                    break;
                };
                vec![job]
            } else {
                match receiver.recv_timeout(deferred_retry_delay(deferred_failures)) {
                    Ok(job) => {
                        let mut batch = std::mem::take(&mut deferred);
                        batch.push(job);
                        batch
                    }
                    Err(RecvTimeoutError::Timeout) => std::mem::take(&mut deferred),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            };
            if !actor_delivery_worker::wait_for_delivery_slot(
                &batch[0],
                &last_delivery,
                &thread_cancelled,
            ) {
                if thread_cancelled.load(Ordering::Acquire) {
                    for job in &batch {
                        release_in_flight(job);
                    }
                    break;
                }
                deferred = batch;
                deferred_failures = deferred_failures.saturating_add(1);
                continue;
            }
            if batch[0].actor.runtime != ActorRuntime::Custom
                && !crate::ops::local_headless::supports(&batch[0].actor)
            {
                if !actor_delivery_worker::interruptible_sleep(BATCH_WINDOW, &thread_cancelled) {
                    for job in &batch {
                        release_in_flight(job);
                    }
                    break;
                }
                while batch.len() < BATCH_CAPACITY {
                    match receiver.try_recv() {
                        Ok(job) => batch.push(job),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            let mut delivered = false;
            for attempt in 0..3 {
                if actor_delivery_worker::process_batch(
                    &batch,
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
                if !actor_delivery_worker::interruptible_sleep(
                    std::time::Duration::from_millis(250 * (attempt + 1)),
                    &thread_cancelled,
                ) {
                    break;
                }
            }
            if !delivered {
                deferred = batch;
                deferred_failures = deferred_failures.saturating_add(1);
            } else {
                deferred_failures = 0;
            }
        }
        for job in &deferred {
            release_in_flight(job);
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

fn deferred_retry_delay(failures: u32) -> std::time::Duration {
    let exponent = failures.saturating_sub(1).min(4);
    let delay = std::time::Duration::from_millis(250 * (1_u64 << exponent));
    delay.min(DEFERRED_RETRY_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::Scope;

    #[test]
    fn delivery_settings_prefer_canonical_section_and_read_legacy_flat_value() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("delivery settings", "").expect("group");
        group
            .extra
            .insert("settings".into(), json!({"min_interval_seconds":2}));
        assert_eq!(
            delivery_setting(&group, "min_interval_seconds").and_then(|value| value.as_u64()),
            Some(2)
        );
        group
            .extra
            .insert("delivery".into(), json!({"min_interval_seconds":7}));
        assert_eq!(
            delivery_setting(&group, "min_interval_seconds").and_then(|value| value.as_u64()),
            Some(7)
        );
    }

    #[test]
    fn actor_activation_can_reload_canonical_unread_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("unread activation", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Custom;
        actor.command = vec!["sh".into(), "-c".into(), "sleep 30".into()];
        group.actors.push(actor);
        store.save(&group).expect("save actor");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":"recover me"})
            .as_object()
            .cloned()
            .expect("event data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &event,
        )
        .expect("append message");

        assert_eq!(dispatch_unread(&home, &group, "peer1"), 1);
        assert!(in_flight().lock().expect("in flight").contains(&(
            group.group_id.clone(),
            "peer1".into(),
            event.id.clone(),
        )));

        shutdown_actor(&group.group_id, "peer1");
        let _ = cccc_runtime::stop(&group.group_id, "peer1");
    }

    #[test]
    fn deferred_delivery_retries_without_waiting_for_another_message() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("deferred retry", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Custom;
        actor.command = vec!["sh".into(), "-c".into(), "sleep 30".into()];
        group.actors.push(actor);
        group.scopes.push(Scope {
            scope_key: "scope".into(),
            url: workspace.to_string_lossy().into_owned(),
            label: "workspace".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "scope".into();
        store.save(&group).expect("save actor");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":"retry without a new event"})
            .as_object()
            .cloned()
            .expect("event data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &event,
        )
        .expect("append message");

        let group_path = store
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("group.yaml");
        let hidden_path = group_path.with_extension("yaml.hidden");
        std::fs::rename(&group_path, &hidden_path).expect("hide group state");
        assert_eq!(dispatch_unread(&home, &group, "peer1"), 1);
        std::thread::sleep(std::time::Duration::from_millis(600));
        std::fs::rename(&hidden_path, &group_path).expect("restore group state");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let completed = completions()
                .lock()
                .expect("completions")
                .iter()
                .any(|item| item.event_id == event.id);
            if completed || std::time::Instant::now() >= deadline {
                assert!(completed, "deferred delivery was not retried");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        shutdown_actor(&group.group_id, "peer1");
        let _ = cccc_runtime::stop(&group.group_id, "peer1");
    }

    #[test]
    fn deferred_retry_backoff_is_bounded() {
        assert_eq!(
            deferred_retry_delay(1),
            std::time::Duration::from_millis(250)
        );
        assert_eq!(deferred_retry_delay(u32::MAX), DEFERRED_RETRY_MAX);
    }
}

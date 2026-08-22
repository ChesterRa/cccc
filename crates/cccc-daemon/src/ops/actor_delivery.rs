use cccc_contracts::{Actor, ActorRuntime, Event, GroupState};
use cccc_core::{GroupDoc, HomeLayout, inbox};
use serde::Serialize;
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
    pub actor_created_at: String,
    pub event_id: String,
    pub transport: String,
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

pub(super) fn complete_job(job: &DeliveryJob) {
    let transport = delivery_transport(&job.home, &job.group, &job.actor);
    match crate::ops::runtime_delivery::append_state(
        &job.home,
        &job.group.group_id,
        &job.actor.id,
        &job.actor.created_at,
        &job.event.id,
        transport,
        crate::ops::runtime_delivery::DeliveryOutcome::Accepted,
    ) {
        Ok(_) => release_in_flight(job),
        Err(error) => {
            tracing::warn!(
                message = %error.message,
                event_id = %job.event.id,
                "runtime accepted delivery but its ledger result is pending"
            );
            record_completion(DeliveryCompletion {
                group_id: job.group.group_id.clone(),
                actor_id: job.actor.id.clone(),
                actor_created_at: job.actor.created_at.clone(),
                event_id: job.event.id.clone(),
                transport: transport.into(),
            });
        }
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

fn fail_job(job: &DeliveryJob, reason: &str) {
    if let Err(error) = crate::ops::runtime_delivery::append_state(
        &job.home,
        &job.group.group_id,
        &job.actor.id,
        &job.actor.created_at,
        &job.event.id,
        delivery_transport(&job.home, &job.group, &job.actor),
        crate::ops::runtime_delivery::DeliveryOutcome::Failed(reason),
    ) {
        tracing::warn!(
            message = %error.message,
            event_id = %job.event.id,
            "failed to record interrupted runtime delivery"
        );
    }
    release_in_flight(job);
}

fn fail_jobs(jobs: &[DeliveryJob], reason: &str) {
    for job in jobs {
        fail_job(job, reason);
    }
}

pub fn dispatch(home: &HomeLayout, group: &GroupDoc, event: &Event) -> DispatchReport {
    if !matches!(event.kind.as_str(), "chat.message" | "system.notify")
        || matches!(group.state, GroupState::Paused | GroupState::Stopped)
    {
        return report(0, 0, 0);
    }

    if event.kind == "chat.message"
        && event
            .data
            .get("message_mode")
            .and_then(serde_json::Value::as_str)
            == Some("mail")
    {
        return mail_report();
    }

    let targets: Vec<_> = group
        .actors
        .iter()
        .filter(|actor| {
            (!crate::ops::actor_runtime::is_structured(actor)
                || crate::ops::local_headless::supports(actor)
                || actor.runtime == ActorRuntime::Deepseek)
                && event_targets_actor(group, event, &actor.id)
        })
        .cloned()
        .collect();
    dispatch_to(home, group, event, &targets, false)
}

fn event_targets_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    if event.kind == "system.notify"
        && matches!(
            event.data.get("kind").and_then(serde_json::Value::as_str),
            Some("mail_notice" | "reply_notice")
        )
    {
        return event
            .data
            .get("target_actor_id")
            .and_then(serde_json::Value::as_str)
            == Some(actor_id);
    }
    inbox::is_for_actor(group, event, actor_id)
}

pub fn dispatch_to(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    targets: &[Actor],
    force_ambiguous: bool,
) -> DispatchReport {
    dispatch_to_inner(home, group, event, targets, force_ambiguous, false)
}

pub fn dispatch_preclaimed(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    targets: &[Actor],
) -> DispatchReport {
    dispatch_to_inner(home, group, event, targets, false, true)
}

fn dispatch_to_inner(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    targets: &[Actor],
    force_ambiguous: bool,
    preclaimed: bool,
) -> DispatchReport {
    if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return report(targets.len(), 0, 0);
    }
    let mut queued = 0;
    let mut online = 0;
    for actor in targets {
        if !actor.enabled {
            continue;
        }
        let actor_online = if actor.runtime == ActorRuntime::Deepseek {
            crate::ops::deepseek_runtime::running(&group.group_id, &actor.id)
        } else if crate::ops::local_headless::supports(actor) {
            crate::ops::local_headless::running(&group.group_id, &actor.id)
        } else {
            cccc_runtime::status(&group.group_id, &actor.id).is_ok_and(|status| status.running)
        };
        if actor_online {
            online += 1;
        }
        let transport = delivery_transport(home, group, actor);
        if preclaimed && actor.runtime == ActorRuntime::WebModel {
            // Structured Web Model consumers take the durable claim through
            // runtime_wait_next_turn. Do not enqueue the actor on the PTY lane.
            queued += 1;
            continue;
        }
        if !preclaimed {
            match crate::ops::runtime_delivery::claim(
                home,
                group,
                actor,
                &event.id,
                transport,
                force_ambiguous,
            ) {
                Ok(crate::ops::runtime_delivery::ClaimResult::Claimed) => {}
                Ok(crate::ops::runtime_delivery::ClaimResult::Terminal(_)) => continue,
                Err(error) => {
                    tracing::warn!(
                        group_id = %group.group_id,
                        actor_id = %actor.id,
                        event_id = %event.id,
                        message = %error.message,
                        "failed to claim runtime delivery"
                    );
                    continue;
                }
            }
        }
        if enqueue(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
        }) {
            queued += 1;
        } else if let Err(error) = crate::ops::runtime_delivery::append_state(
            home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            &event.id,
            transport,
            crate::ops::runtime_delivery::DeliveryOutcome::Failed(
                "daemon delivery queue did not accept the event",
            ),
        ) {
            tracing::warn!(message = %error.message, "failed to record rejected runtime delivery");
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
            && !crate::ops::local_headless::supports(actor)
            && actor.runtime != ActorRuntime::Deepseek)
    {
        return 0;
    }
    let events = match crate::ops::runtime_delivery::pending_sources(
        home,
        group,
        actor,
        QUEUE_CAPACITY,
    ) {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(message = %error.message, %actor_id, "failed to load pending runtime deliveries");
            return 0;
        }
    };
    events
        .into_iter()
        .map(|event| dispatch_to(home, group, &event, std::slice::from_ref(actor), false).queued)
        .sum()
}

pub fn dispatch_group_unread(home: &HomeLayout, group: &GroupDoc) -> usize {
    group
        .actors
        .iter()
        .map(|actor| dispatch_unread(home, group, &actor.id))
        .sum()
}

pub fn mail_report() -> DispatchReport {
    DispatchReport {
        accepted: true,
        state: "mail",
        targeted: 0,
        online: 0,
        queued: 0,
    }
}

pub(super) fn delivery_transport(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> &'static str {
    if actor.runtime == ActorRuntime::Deepseek {
        "deepseek"
    } else if actor.runtime == ActorRuntime::WebModel {
        web_model_delivery_transport(home, group, actor)
    } else if crate::ops::local_headless::supports(actor) {
        "local_headless"
    } else {
        "pty"
    }
}

fn web_model_delivery_transport(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> &'static str {
    let setting = |names: &[&str]| {
        names
            .iter()
            .filter_map(|name| actor.env.get(*name))
            .map(|value| value.trim().to_ascii_lowercase())
            .find(|value| !value.is_empty())
            .or_else(|| {
                names
                    .iter()
                    .filter_map(|name| std::env::var(name).ok())
                    .map(|value| value.trim().to_ascii_lowercase())
                    .find(|value| !value.is_empty())
            })
            .unwrap_or_default()
    };
    let mode = setting(&["CCCC_WEB_MODEL_DELIVERY_MODE", "CCCC_WEB_MODEL_DELIVERY"]);
    if matches!(
        mode.as_str(),
        "pull" | "native" | "remote_mcp" | "off" | "disabled" | "none"
    ) {
        return "web_model_pull";
    }
    if matches!(
        mode.as_str(),
        "browser" | "chatgpt" | "chatgpt_browser" | "browser_delivery"
    ) {
        return "web_model_browser";
    }
    let mut provider = setting(&["CCCC_WEB_MODEL_PROVIDER", "CCCC_WEB_MODEL_BROWSER_PROVIDER"]);
    if provider.is_empty() {
        provider = cccc_core::web_model_connectors::load(home)
            .unwrap_or_default()
            .into_iter()
            .find(|connector| {
                !connector["revoked"].as_bool().unwrap_or(false)
                    && connector["group_id"].as_str() == Some(group.group_id.as_str())
                    && connector["actor_id"].as_str() == Some(actor.id.as_str())
            })
            .and_then(|connector| connector["provider"].as_str().map(str::to_owned))
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
    }
    if matches!(
        provider.as_str(),
        "chatgpt" | "chatgpt_web" | "browser_web_model" | "chatgpt_browser"
    ) {
        "web_model_browser"
    } else {
        "web_model_pull"
    }
}

fn report(targeted: usize, online: usize, queued: usize) -> DispatchReport {
    let state = if queued > 0 {
        "queued"
    } else if online > 0 {
        "queue_full"
    } else if targeted > 0 {
        "blocked"
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
                    fail_jobs(&batch, "delivery worker stopped before runtime acceptance");
                    break;
                }
                deferred = batch;
                deferred_failures = deferred_failures.saturating_add(1);
                continue;
            }
            if batch[0].actor.runtime != ActorRuntime::Custom
                && batch[0].actor.runtime != ActorRuntime::Deepseek
                && !crate::ops::local_headless::supports(&batch[0].actor)
            {
                if !actor_delivery_worker::interruptible_sleep(BATCH_WINDOW, &thread_cancelled) {
                    fail_jobs(&batch, "delivery worker stopped before runtime acceptance");
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
        fail_jobs(
            &deferred,
            "delivery worker stopped before runtime acceptance",
        );
        for job in receiver.try_iter() {
            fail_job(&job, "delivery worker stopped before runtime acceptance");
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
    use cccc_core::{GroupStore, ledger};
    use serde_json::json;

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
    fn mail_is_stored_without_runtime_queueing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("mail", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor);
        store.save(&group).expect("save actor");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":"read later","message_mode":"mail"})
            .as_object()
            .cloned()
            .expect("event data");
        let report = dispatch(&home, &group, &event);
        assert_eq!(report.state, "mail");
        assert_eq!(report.queued, 0);
        assert!(in_flight().lock().expect("in flight").is_empty());
    }

    #[test]
    fn paused_direct_delivery_is_blocked_not_reported_as_mail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("paused direct", "").expect("group");
        group.state = GroupState::Paused;
        group.actors.push(Actor::new("peer1"));
        store.save(&group).expect("save actor");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":"wait","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("event data");

        let report = dispatch_to(&home, &group, &event, &group.actors, false);
        assert_eq!(report.state, "blocked");
        assert_eq!(report.queued, 0);
    }

    #[test]
    fn deferred_retry_backoff_is_bounded() {
        assert_eq!(
            deferred_retry_delay(1),
            std::time::Duration::from_millis(250)
        );
        assert_eq!(deferred_retry_delay(u32::MAX), DEFERRED_RETRY_MAX);
    }

    #[test]
    fn worker_shutdown_releases_claims_as_retryable_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("paused delivery", "").expect("group");
        group.state = GroupState::Paused;
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");

        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer1"],"text":"retry me","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("event data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger path"),
            &event,
        )
        .expect("append source");
        crate::ops::runtime_delivery::append_state(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            &event.id,
            delivery_transport(&home, &group, &actor),
            crate::ops::runtime_delivery::DeliveryOutcome::Claimed,
        )
        .expect("claim");

        assert!(enqueue(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
        }));
        shutdown_actor(&group.group_id, &actor.id);

        assert_eq!(
            crate::ops::runtime_delivery::latest_state(
                &home,
                &group.group_id,
                &actor.id,
                &event.id,
            )
            .expect("latest state")
            .expect("delivery state")
            .0,
            "failed"
        );
        assert!(in_flight().lock().expect("in flight").is_empty());
    }
}

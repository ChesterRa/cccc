use cccc_contracts::{Actor, ActorRuntime, Event, GroupState};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, inbox, ledger};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

use crate::ops::actor_delivery_worker;

const QUEUE_CAPACITY: usize = 256;

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

fn workers() -> &'static Mutex<HashMap<Key, SyncSender<DeliveryJob>>> {
    static WORKERS: OnceLock<Mutex<HashMap<Key, SyncSender<DeliveryJob>>>> = OnceLock::new();
    WORKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn completions() -> &'static Mutex<VecDeque<DeliveryCompletion>> {
    static COMPLETIONS: OnceLock<Mutex<VecDeque<DeliveryCompletion>>> = OnceLock::new();
    COMPLETIONS.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(super) fn record_completion(completion: DeliveryCompletion) {
    if let Ok(mut queue) = completions().lock() {
        queue.push_back(completion);
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
            actor.enabled
                && actor.runtime != ActorRuntime::WebModel
                && inbox::is_for_actor(group, event, &actor.id)
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
    for completion in pending {
        let Ok(group) = store.load(&completion.group_id) else {
            continue;
        };
        if !auto_mark_on_delivery(&group)
            || !inbox::advance(
                home,
                &completion.group_id,
                &completion.actor_id,
                &completion.event_id,
            )
            .unwrap_or(false)
        {
            continue;
        }
        let mut event = Event::new("chat.read", &completion.group_id);
        event.by.clone_from(&completion.actor_id);
        event.data = json!({
            "actor_id": completion.actor_id,
            "event_id": completion.event_id,
            "source": "runtime_delivery",
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        if let Ok(path) = store.ledger_path(&completion.group_id) {
            let _ = ledger::append(&path, &event);
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
    let mut map = match workers().lock() {
        Ok(map) => map,
        Err(_) => return false,
    };
    let sender = map.entry(key.clone()).or_insert_with(|| spawn_worker(&key));
    match sender.try_send(job) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            tracing::warn!(group_id = %key.0, actor_id = %key.1, "PTY delivery queue is full");
            false
        }
        Err(TrySendError::Disconnected(job)) => {
            let sender = spawn_worker(&key);
            let result = sender.try_send(job).is_ok();
            map.insert(key, sender);
            result
        }
    }
}

fn spawn_worker(key: &Key) -> SyncSender<DeliveryJob> {
    let (sender, receiver) = mpsc::sync_channel::<DeliveryJob>(QUEUE_CAPACITY);
    let name = format!("cccc-delivery:{}:{}", key.0, key.1);
    if let Err(error) = std::thread::Builder::new().name(name).spawn(move || {
        let mut preamble_session = String::new();
        let mut last_delivery = None;
        while let Ok(job) = receiver.recv() {
            actor_delivery_worker::process(job, &mut preamble_session, &mut last_delivery);
        }
    }) {
        tracing::warn!(%error, "failed to start actor delivery worker");
    }
    sender
}

fn auto_mark_on_delivery(group: &GroupDoc) -> bool {
    group
        .extra
        .get("settings")
        .and_then(|value| value.get("auto_mark_on_delivery"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

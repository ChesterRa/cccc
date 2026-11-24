use cccc_contracts::event::DeliveryEvidence;
use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, Event, GroupState};
use cccc_core::{GroupDoc, HomeLayout, inbox};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};

use crate::ops::actor_delivery_worker;

mod drain;
mod gaps;
mod lifecycle;
mod terminal;
#[cfg(test)]
pub(crate) use drain::drain_group;
pub(crate) use drain::{
    PreparedDrain, apply_drain, drain_local_completions, pending_group_ids, prepare_drain,
};
pub use lifecycle::{
    ShutdownReason, pause_group, shutdown_actor, shutdown_actor_for, shutdown_all, shutdown_group,
};
pub(crate) use terminal::{submit_terminal_text_for_session, submit_viewer_text};

const QUEUE_CAPACITY: usize = 256;
const COMPLETION_CAPACITY: usize = 4096;
const BATCH_CAPACITY: usize = 64;
const DEFERRED_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(4);
/// Retries of a turn the provider ran and failed: after the first turn and
/// this many more, the delivery is settled Failed with the provider's error.
const FAILED_TURN_RETRIES: u32 = 3;
const TERMINAL_SUBMIT_DELAY: std::time::Duration = std::time::Duration::from_millis(1_500);
const TERMINAL_REPEAT_SUBMIT_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

type Key = (String, String);
type DeliveryKey = (String, String, String);

pub(super) fn terminal_submit_sequence(actor: &Actor) -> &'static [&'static [u8]] {
    match actor.submit {
        ActorSubmit::Enter
            if matches!(actor.runtime, ActorRuntime::Codex | ActorRuntime::Copilot) =>
        {
            &[b"\r", b"\r"]
        }
        ActorSubmit::Enter => &[b"\r"],
        ActorSubmit::Newline => &[b"\n"],
        ActorSubmit::None => &[],
    }
}

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
    /// Native waiting work owns only its in-memory slot until handoff is ready.
    pub claimed: bool,
    /// The channel that claimed this work. Its outcome is recorded here, even
    /// if the Actor's owner changed since.
    pub transport: &'static str,
}

/// A delivery outcome whose ledger record is still pending. It holds the
/// in-flight slot until recorded, so the event is never dispatched again.
pub(super) struct DeliveryCompletion {
    pub group_id: String,
    pub actor_id: String,
    pub actor_created_at: String,
    pub event_id: String,
    pub transport: String,
    pub outcome: CompletedDelivery,
}

impl DeliveryCompletion {
    fn persist(&self, home: &HomeLayout) -> Result<Event, crate::dispatch::OpError> {
        crate::ops::runtime_delivery::append_state_with_owner(
            home,
            &self.group_id,
            &self.actor_id,
            &self.actor_created_at,
            &self.event_id,
            &self.transport,
            self.outcome.ledger_outcome(),
            crate::ops::runtime_delivery::OwnerAcknowledgement::AfterWritePermit {
                known_only: true,
            },
        )
    }
}

pub(super) enum CompletedDelivery {
    Accepted(Option<DeliveryEvidence>),
    Withdrawn(String),
    Failed(String),
    Unsubmitted(String),
    Ambiguous(String),
}

impl CompletedDelivery {
    fn ledger_outcome(&self) -> crate::ops::runtime_delivery::DeliveryOutcome<'_> {
        use crate::ops::runtime_delivery::DeliveryOutcome;
        match self {
            Self::Accepted(evidence) => DeliveryOutcome::Accepted(*evidence),
            Self::Withdrawn(reason) => DeliveryOutcome::Withdrawn(reason),
            Self::Failed(reason) => DeliveryOutcome::Failed(reason),
            Self::Unsubmitted(reason) => DeliveryOutcome::Unsubmitted(reason),
            Self::Ambiguous(reason) => DeliveryOutcome::Ambiguous(reason),
        }
    }
}

struct DeliveryWorker {
    sender: Option<SyncSender<DeliveryJob>>,
    cancelled: Arc<AtomicBool>,
    /// Set before cancellation when the Actor's runtime owner is replaced.
    replaced: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DeliveryWorker {
    fn shutdown(self) {
        self.shutdown_for(lifecycle::ShutdownReason::Stop);
    }

    fn shutdown_for(mut self, reason: lifecycle::ShutdownReason) {
        if reason == lifecycle::ShutdownReason::Replace {
            self.replaced.store(true, Ordering::Release);
        }
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

/// How an attempt ended, until the Group's maintenance tells the sender. It keeps
/// IDs, not the message: a full queue must stay small.
struct SenderNotice {
    group_id: String,
    event_id: String,
    recipient: String,
    delivery_id: String,
    uncertain: bool,
}

fn sender_notices() -> &'static Mutex<VecDeque<SenderNotice>> {
    static NOTICES: OnceLock<Mutex<VecDeque<SenderNotice>>> = OnceLock::new();
    NOTICES.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn in_flight() -> &'static Mutex<HashSet<DeliveryKey>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<DeliveryKey>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A queue slot owned by the caller until it reaches a worker.
struct DeliverySlot(Option<DeliveryKey>);

impl DeliverySlot {
    fn reserve(group_id: &str, actor_id: &str, event_id: &str) -> Result<Self, Offer> {
        let key = (
            group_id.to_owned(),
            actor_id.to_owned(),
            event_id.to_owned(),
        );
        let mut pending = in_flight().lock().map_err(|_| Offer::Refused)?;
        if pending.insert(key.clone()) {
            Ok(Self(Some(key)))
        } else {
            Err(Offer::AlreadyInFlight)
        }
    }

    fn transfer(mut self) {
        self.0 = None;
    }
}

impl Drop for DeliverySlot {
    fn drop(&mut self) {
        if let Some(key) = &self.0
            && let Ok(mut pending) = in_flight().lock()
        {
            pending.remove(key);
        }
    }
}

fn fenced() -> &'static Mutex<HashSet<Key>> {
    static FENCED: OnceLock<Mutex<HashSet<Key>>> = OnceLock::new();
    FENCED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// External-session members needing a later unread refill, kept independently of
/// live workers so pre-enqueue read failures survive even before a worker spawns.
/// Value is `paused`.
fn refills() -> &'static Mutex<HashMap<Key, bool>> {
    static REFILLS: OnceLock<Mutex<HashMap<Key, bool>>> = OnceLock::new();
    REFILLS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Groups whose `GroupDoc` reload failed after committing a notification, awaiting
/// maintenance catch-up. Value is `(paused, revision)`.
fn group_catchups() -> &'static Mutex<HashMap<String, (bool, u64)>> {
    static GROUP_CATCHUPS: OnceLock<Mutex<HashMap<String, (bool, u64)>>> = OnceLock::new();
    GROUP_CATCHUPS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(in crate::ops) fn set_group_paused(group_id: &str, paused: bool) {
    gaps::set_group_paused(group_id, paused);
    if let Ok(mut pending) = refills().lock() {
        for ((group, _), flag) in pending.iter_mut() {
            if group == group_id {
                *flag = paused;
            }
        }
    }
    if let Ok(mut pending) = group_catchups().lock()
        && let Some((flag, _)) = pending.get_mut(group_id)
    {
        *flag = paused;
    }
}

/// Schedule a whole-history catch-up for one member after a pre-enqueue failure
/// (e.g. pending read error, restore failure, or >queue-capacity backlog).
pub(in crate::ops) fn record_member_catchup(group: &GroupDoc, actor: &Actor) {
    if matches!(group.state, GroupState::Paused | GroupState::Stopped)
        || !actor.enabled
        || held_for_switch(group, actor)
    {
        return;
    }
    if actor.external_session_id.is_empty() {
        gaps::record(group, actor, None);
    } else {
        set_refill(&group.group_id, &actor.id, true);
    }
}

/// Schedule a group-level catch-up when a notification was appended to the ledger
/// but the group document could not be reloaded to dispatch it immediately.
pub(in crate::ops) fn record_group_catchup(group_id: &str) {
    if let Ok(mut pending) = group_catchups().lock() {
        let revision = pending
            .get(group_id)
            .map_or(0, |(_, rev)| rev.wrapping_add(1));
        pending.insert(group_id.to_owned(), (false, revision));
    }
}

fn active_group_catchup_revision(group_id: &str) -> Option<u64> {
    group_catchups()
        .lock()
        .ok()
        .and_then(|pending| pending.get(group_id).copied())
        .filter(|(paused, _)| !paused)
        .map(|(_, revision)| revision)
}

fn complete_group_catchup(group_id: &str, revision: u64) {
    if let Ok(mut pending) = group_catchups().lock()
        && pending
            .get(group_id)
            .is_some_and(|(_, current)| *current == revision)
    {
        pending.remove(group_id);
    }
}

fn clear_group_catchup(group_id: &str) {
    if let Ok(mut pending) = group_catchups().lock() {
        pending.remove(group_id);
    }
}

fn clear_all_group_catchups() {
    if let Ok(mut pending) = group_catchups().lock() {
        pending.clear();
    }
}

fn collect_group_catchup_groups(groups: &mut HashSet<String>) {
    if let Ok(pending) = group_catchups().lock() {
        groups.extend(
            pending
                .iter()
                .filter(|(_, (paused, _))| !paused)
                .map(|(group_id, _)| group_id.clone()),
        );
    }
}

fn clear_refills(remove: impl Fn(&Key) -> bool) {
    if let Ok(mut pending) = refills().lock() {
        pending.retain(|key, _| !remove(key));
    }
}

fn collect_refill_groups(groups: &mut HashSet<String>) {
    if let Ok(pending) = refills().lock() {
        groups.extend(
            pending
                .iter()
                .filter(|(_, paused)| !**paused)
                .map(|((group_id, _), _)| group_id.clone()),
        );
    }
}

fn pending_refill_actors(group_id: &str) -> Vec<String> {
    refills()
        .lock()
        .map(|pending| {
            pending
                .iter()
                .filter(|((id, _), paused)| id == group_id && !**paused)
                .map(|((_, actor_id), _)| actor_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Keeps new delivery work away from one Actor while its runtime owner is
/// replaced. Dropping the guard lifts the fence.
pub struct DeliveryFence(Key);

impl Drop for DeliveryFence {
    fn drop(&mut self) {
        // Lift under the worker map, as it was raised: every refusal the
        // fence caused has been settled by then.
        let _workers = workers().lock();
        if let Ok(mut fenced) = fenced().lock() {
            fenced.remove(&self.0);
        }
    }
}

pub fn fence_actor(group_id: &str, actor_id: &str) -> DeliveryFence {
    let key = (group_id.to_owned(), actor_id.to_owned());
    // Fence under the worker map: a racing enqueue either created its worker
    // before (the shutdown that follows stops it) or sees the fence.
    let _workers = workers().lock();
    if let Ok(mut fenced) = fenced().lock() {
        fenced.insert(key.clone());
    }
    DeliveryFence(key)
}

fn is_fenced(group_id: &str, actor_id: &str) -> bool {
    fenced()
        .lock()
        .is_ok_and(|fenced| fenced.contains(&(group_id.to_owned(), actor_id.to_owned())))
}

/// New work waits while the Actor's runtime is being replaced, or until an
/// unfinished switch has written its handoff.
fn held_for_switch(group: &GroupDoc, actor: &Actor) -> bool {
    is_fenced(&group.group_id, &actor.id)
        || cccc_core::runtime_switches::pending_for(group, actor).is_some()
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

pub(super) fn complete_job(job: &DeliveryJob, evidence: Option<DeliveryEvidence>) {
    finish_job(job, CompletedDelivery::Accepted(evidence));
}

pub(super) fn finish_job(job: &DeliveryJob, outcome: CompletedDelivery) {
    finish_job_with_owner(
        job,
        outcome,
        crate::ops::runtime_delivery::OwnerAcknowledgement::KnownAttempt,
    );
}

fn finish_job_with_owner(
    job: &DeliveryJob,
    outcome: CompletedDelivery,
    owner: crate::ops::runtime_delivery::OwnerAcknowledgement,
) {
    match write_job_state(job, outcome.ledger_outcome(), owner) {
        Ok(_) => release_in_flight(job),
        Err(error) => {
            tracing::warn!(
                message = %error.message,
                event_id = %job.event.id,
                "runtime delivery outcome is awaiting its ledger record"
            );
            record_completion(DeliveryCompletion {
                group_id: job.group.group_id.clone(),
                actor_id: job.actor.id.clone(),
                actor_created_at: job.actor.created_at.clone(),
                event_id: job.event.id.clone(),
                transport: job.transport.into(),
                outcome,
            });
        }
    }
}

pub(super) fn append_job_state(
    job: &DeliveryJob,
    outcome: crate::ops::runtime_delivery::DeliveryOutcome<'_>,
) -> Result<Event, crate::dispatch::OpError> {
    write_job_state(
        job,
        outcome,
        crate::ops::runtime_delivery::OwnerAcknowledgement::KnownAttempt,
    )
}

fn write_job_state(
    job: &DeliveryJob,
    outcome: crate::ops::runtime_delivery::DeliveryOutcome<'_>,
    owner: crate::ops::runtime_delivery::OwnerAcknowledgement,
) -> Result<Event, crate::dispatch::OpError> {
    crate::ops::runtime_delivery::append_state_with_owner(
        &job.home,
        &job.group.group_id,
        &job.actor.id,
        &job.actor.created_at,
        &job.event.id,
        job.transport,
        outcome,
        owner,
    )
}

fn clear_in_flight(mut remove: impl FnMut(&DeliveryKey) -> bool) {
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
    settle_failure(
        job,
        crate::ops::runtime_delivery::DeliveryOutcome::Failed(reason),
    );
}

/// The runtime ran this delivery and its bounded retries failed too: the failure
/// stays visible, and only an explicit delivery request starts it again.
fn exhaust_job(job: &DeliveryJob, reason: &str) {
    settle_failure(
        job,
        crate::ops::runtime_delivery::DeliveryOutcome::RetriesExhausted(reason),
    );
}

fn settle_failure(job: &DeliveryJob, outcome: crate::ops::runtime_delivery::DeliveryOutcome<'_>) {
    if let Err(error) = append_job_state(job, outcome) {
        tracing::warn!(
            message = %error.message,
            event_id = %job.event.id,
            "failed to record interrupted runtime delivery"
        );
    }
    release_in_flight(job);
}

/// Close deliveries that were never sent and must not be retried. A failed
/// record is queued like an accepted one: only the record is retried, never
/// the submission, and the event keeps its in-flight slot meanwhile.
pub(super) fn withdraw_jobs(jobs: &[DeliveryJob], reason: &str) {
    for job in jobs {
        match append_job_state(
            job,
            crate::ops::runtime_delivery::DeliveryOutcome::Withdrawn(reason),
        ) {
            Ok(_) => release_in_flight(job),
            Err(error) => {
                tracing::warn!(message = %error.message, event_id = %job.event.id, "withdrawn delivery record is pending");
                record_completion(DeliveryCompletion {
                    group_id: job.group.group_id.clone(),
                    actor_id: job.actor.id.clone(),
                    actor_created_at: job.actor.created_at.clone(),
                    event_id: job.event.id.clone(),
                    transport: job.transport.into(),
                    outcome: CompletedDelivery::Withdrawn(reason.into()),
                });
            }
        }
    }
}

pub(super) fn fail_jobs(jobs: &[DeliveryJob], reason: &str) {
    for job in jobs {
        fail_job(job, reason);
        tell_sender(job, false);
    }
}

/// Close work that will not be retried on its own. What may have reached the
/// runtime stays uncertain: calling it undelivered would invite a second copy.
fn close_jobs(jobs: &[DeliveryJob], attempted: &HashSet<String>, reason: &str) {
    for job in jobs {
        let uncertain = attempted.contains(&job.event.id);
        if uncertain {
            finish_job(
                job,
                CompletedDelivery::Ambiguous(format!(
                    "This delivery may have been submitted before it stopped: {reason}"
                )),
            );
        } else {
            fail_job(job, reason);
        }
        tell_sender(job, uncertain);
    }
}

/// Leave word for the agent that sent this work how this attempt ended: only the
/// Web shows that otherwise. Mail is left out, because it stays in the Inbox
/// whatever became of a push. A worker cannot write to the Group itself: lifecycle
/// operations hold the Group and wait for the worker.
pub(super) fn tell_sender(job: &DeliveryJob, uncertain: bool) {
    let by = job.event.by.as_str();
    let mode = job.event.data.get("message_mode");
    let mail = mode.and_then(serde_json::Value::as_str) == Some("mail");
    if job.event.kind != "chat.message"
        || mail
        || matches!(by, "user" | "system")
        || by == job.actor.id
    {
        return;
    }
    let notice = SenderNotice {
        group_id: job.group.group_id.clone(),
        event_id: job.event.id.clone(),
        recipient: job.actor.id.clone(),
        delivery_id: crate::ops::runtime_delivery::delivery_id(
            &job.group.group_id,
            &job.actor.id,
            &job.actor.created_at,
            &job.event.id,
        ),
        uncertain,
    };
    if let Ok(mut queue) = sender_notices().lock() {
        if queue.len() >= COMPLETION_CAPACITY {
            queue.pop_front();
            tracing::warn!("sender notice queue reached capacity");
        }
        queue.push_back(notice);
    }
}

/// Tells the member who asked Chet in a Group, once, that the host never sent
/// the request. The caller holds the Group's write permit, so the notice is
/// written before it reports success and a retry cannot repeat it.
pub(super) fn tell_sender_not_sent_to_chet(
    home: &HomeLayout,
    group_id: &str,
    event_id: &str,
    delivery_id: &str,
) -> Result<(), crate::dispatch::OpError> {
    drain::notify_sender(
        home,
        &SenderNotice {
            group_id: group_id.to_owned(),
            event_id: event_id.to_owned(),
            recipient: cccc_core::actors::CHET_SERVICE_PRINCIPAL.to_owned(),
            delivery_id: delivery_id.to_owned(),
            uncertain: false,
        },
    )
}

/// The caller just appended this source. No retained execution has seen it.
pub fn dispatch(home: &HomeLayout, group: &GroupDoc, event: &Event) -> DispatchReport {
    dispatch_with_origin(home, group, event, DispatchSource::New(None))
}

pub(super) fn dispatch_observed_notice(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    observed: &crate::session_owner::OwnerObservations,
) {
    dispatch_with_origin(home, group, event, DispatchSource::New(Some(observed)));
}

fn dispatch_with_origin(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    source: DispatchSource<'_>,
) -> DispatchReport {
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

    let targets = group
        .actors
        .iter()
        .filter(|actor| event_targets_actor(group, event, &actor.id))
        .filter(|actor| {
            if configured_for_push(actor) {
                return true;
            }
            let observed = if source.uses_live_target() {
                crate::session_owner::running_target(home, &group.group_id, actor).and_then(
                    |target| {
                        if target.is_some() {
                            Ok(true)
                        } else {
                            running_push_owner(group, actor)
                        }
                    },
                )
            } else {
                source.observe(|| running_push_owner(group, actor))
            };
            match observed {
                Ok(running) => running,
                Err(_) => {
                    if actor.enabled && !held_for_switch(group, actor) {
                        gaps::record(group, actor, Some(event));
                    }
                    false
                }
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    dispatch_to_inner(home, group, event, &targets, false, None, source)
}

/// Whether work reaches this Actor by push rather than by its own pull. What
/// runs now decides; saved settings apply when nothing does.
pub(super) fn receives_push(group: &GroupDoc, actor: &Actor) -> bool {
    configured_for_push(actor) || running_push_owner(group, actor).unwrap_or(false)
}

fn configured_for_push(actor: &Actor) -> bool {
    !actor.external_session_id.is_empty()
        || !crate::ops::actor_runtime::is_structured(actor)
        || crate::ops::local_headless::supports(actor)
        || actor.runtime == ActorRuntime::Deepseek
}

fn running_push_owner(group: &GroupDoc, actor: &Actor) -> std::io::Result<bool> {
    use crate::ops::actor_runtime::{OwnerState, delivery_owner};
    Ok(delivery_owner(&group.group_id, &actor.id)?
        .is_some_and(|owner| owner.state == OwnerState::Running))
}

pub(super) fn event_targets_actor(group: &GroupDoc, event: &Event, actor_id: &str) -> bool {
    if inbox::is_targeted_prompt(event) {
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
    dispatch_to_inner(
        home,
        group,
        event,
        targets,
        force_ambiguous,
        None,
        DispatchSource::Directed,
    )
}

pub(super) struct ExplicitDeliveryTargets<'a> {
    routes: &'a [(&'a Actor, &'static str)],
    slots: HashMap<String, DeliverySlot>,
}

/// Reserve all worker slots before writing an explicit claim. A worker that
/// already holds a slot may still be waiting to take its own claim.
pub(super) fn reserve_explicit_deliveries<'a>(
    group: &GroupDoc,
    event: &Event,
    routes: &'a [(&'a Actor, &'static str)],
) -> Result<ExplicitDeliveryTargets<'a>, crate::dispatch::OpError> {
    use crate::dispatch::OpError;
    use serde_json::json;

    let mut slots = HashMap::new();
    for (actor, transport) in routes {
        if held_for_switch(group, actor) || is_fenced(&group.group_id, &actor.id) {
            let mut error = OpError::new(
                "delivery_blocked",
                format!("actor is switching runtime: {}", actor.id),
            );
            error.details.insert("actor_id".into(), json!(actor.id));
            error
                .details
                .insert("reason".into(), json!("runtime_switch"));
            return Err(error);
        }
        // The Web Model runtime consumes this claim through wait_next_turn.
        if transport.starts_with("web_model") {
            continue;
        }
        let slot = match DeliverySlot::reserve(&group.group_id, &actor.id, &event.id) {
            Ok(slot) => slot,
            Err(Offer::AlreadyInFlight) => {
                let mut error = OpError::new(
                    "delivery_in_progress",
                    format!("message is already queued for actor: {}", actor.id),
                );
                error.details.insert("actor_id".into(), json!(actor.id));
                return Err(error);
            }
            Err(_) => {
                return Err(OpError::io(std::io::Error::other(
                    "delivery queue unavailable",
                )));
            }
        };
        slots.insert(actor.id.clone(), slot);
    }
    Ok(ExplicitDeliveryTargets { routes, slots })
}

/// Offer work its caller already claimed, each on the channel that claimed it.
pub(super) fn dispatch_preclaimed(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    claims: ExplicitDeliveryTargets<'_>,
) -> DispatchReport {
    let targets = claims
        .routes
        .iter()
        .map(|(actor, _)| (*actor).clone())
        .collect::<Vec<_>>();
    dispatch_to_inner(
        home,
        group,
        event,
        &targets,
        false,
        Some(claims),
        DispatchSource::Directed,
    )
}

#[derive(Clone, Copy)]
enum DispatchSource<'a> {
    New(Option<&'a crate::session_owner::OwnerObservations>),
    Unread(&'static str),
    Directed,
}
impl DispatchSource<'_> {
    fn uses_live_target(self) -> bool {
        matches!(self, Self::New(None))
    }

    fn observe<T>(self, read: impl FnOnce() -> T) -> T {
        match self {
            Self::New(Some(observed)) => observed.with_scope(read),
            _ => read(),
        }
    }

    fn missed(self, group: &GroupDoc, actor: &Actor, event: &Event) {
        match self {
            Self::New(_) => {
                if actor.external_session_id.is_empty() {
                    gaps::record(group, actor, Some(event));
                } else {
                    set_refill(&group.group_id, &actor.id, true);
                }
            }
            Self::Unread(_) => record_member_catchup(group, actor),
            Self::Directed => {}
        }
    }
}

struct DispatchOutcome {
    report: DispatchReport,
    interrupted: bool,
    held: bool,
}

fn dispatch_to_inner(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    targets: &[Actor],
    force_ambiguous: bool,
    claims: Option<ExplicitDeliveryTargets<'_>>,
    source: DispatchSource<'_>,
) -> DispatchReport {
    dispatch_to_inner_outcome(home, group, event, targets, force_ambiguous, claims, source).report
}

fn dispatch_to_inner_outcome(
    home: &HomeLayout,
    group: &GroupDoc,
    event: &Event,
    targets: &[Actor],
    force_ambiguous: bool,
    mut claims: Option<ExplicitDeliveryTargets<'_>>,
    source: DispatchSource<'_>,
) -> DispatchOutcome {
    if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        return DispatchOutcome {
            report: report(targets.len(), 0, 0),
            interrupted: false,
            held: false,
        };
    }
    let mut queued = 0;
    let mut online = 0;
    let mut interrupted = false;
    let mut held = false;
    let preclaimed = claims.is_some();
    for actor in targets {
        if !actor.enabled || held_for_switch(group, actor) {
            continue;
        }
        let target = if source.uses_live_target() && actor.external_session_id.is_empty() {
            match crate::session_owner::running_target(home, &group.group_id, actor) {
                Ok(target) => target,
                Err(error) => {
                    tracing::debug!(%error, actor_id = %actor.id, "delivery target is not observable");
                    source.missed(group, actor, event);
                    interrupted = true;
                    continue;
                }
            }
        } else {
            None
        };
        let observe_pause = || {
            source.observe(|| {
                crate::ops::actor_runtime::confirmation_pause_applies(home, &group.group_id, actor)
            })
        };
        // Leave new events pending while this managed Claude awaits an explicit
        // retry. Already claimed work must reach the worker to be settled safely.
        if !preclaimed
            && actor.external_session_id.is_empty()
            && target
                .as_ref()
                .map_or_else(observe_pause, |target| target.observe(observe_pause))
                .is_ok_and(|paused| paused)
        {
            continue;
        }
        if matches!(source, DispatchSource::New(_)) && gaps::extend(group, actor, event) {
            continue;
        }
        let actor_online = if matches!(source, DispatchSource::Unread(_)) {
            // The catch-up caller uses only the queued count.
            false
        } else if !actor.external_session_id.is_empty() {
            super::local_sessions::connected(home, &actor.external_session_id)
        } else {
            target.is_some()
                || source
                    .observe(|| running_push_owner(group, actor))
                    .unwrap_or(false)
        };
        if actor_online {
            online += 1;
        }
        let claimed_on = claims.as_ref().and_then(|claims| {
            claims
                .routes
                .iter()
                .find(|(claimed, _)| claimed.id == actor.id)
                .map(|(_, transport)| *transport)
        });
        let known_transport = match source {
            DispatchSource::Unread(transport) => Some(transport),
            _ => claimed_on,
        };
        let observe_transport = || source.observe(|| delivery_transport(home, group, actor));
        let transport = match known_transport.map_or_else(
            || {
                target.as_ref().map_or_else(observe_transport, |target| {
                    target.observe(observe_transport)
                })
            },
            Ok,
        ) {
            Ok(transport) => transport,
            Err(error) => {
                tracing::warn!(
                    %error,
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    "failed to read the Actor's runtime owner"
                );
                source.missed(group, actor, event);
                interrupted = true;
                continue;
            }
        };
        if preclaimed && transport.starts_with("web_model") {
            // Structured Web Model consumers take the durable claim through
            // runtime_wait_next_turn. Do not enqueue the actor on the PTY lane.
            queued += 1;
            continue;
        }
        let claimed = preclaimed || actor.external_session_id.is_empty() || force_ambiguous;
        let slot = if let Some(claims) = &mut claims {
            claims.slots.remove(&actor.id)
        } else if claimed && matches!(source, DispatchSource::Unread(_)) {
            // A live managed worker may be about to reclaim a failed attempt.
            // The unread scan cannot take that claim away from its consumer.
            match DeliverySlot::reserve(&group.group_id, &actor.id, &event.id) {
                Ok(slot) => Some(slot),
                Err(Offer::AlreadyInFlight) => continue,
                Err(_) => {
                    source.missed(group, actor, event);
                    interrupted = true;
                    continue;
                }
            }
        } else {
            None
        };
        if claimed && !preclaimed {
            use crate::ops::runtime_delivery::{ClaimResult, claim, claim_new_source};
            let result = if matches!(source, DispatchSource::New(_)) {
                claim_new_source(home, group, actor, &event.id, transport)
            } else {
                claim(home, group, actor, &event.id, transport, force_ambiguous)
            };
            match result {
                Ok(ClaimResult::Claimed) => {}
                Ok(ClaimResult::Terminal(_)) => continue,
                Err(error) => {
                    tracing::warn!(
                        group_id = %group.group_id,
                        actor_id = %actor.id,
                        event_id = %event.id,
                        message = %error.message,
                        "failed to claim runtime delivery"
                    );
                    source.missed(group, actor, event);
                    interrupted = true;
                    continue;
                }
            }
        }
        match offer_delivery(home, group, actor, event, transport, claimed, slot) {
            Offer::Queued => queued += 1,
            Offer::AlreadyInFlight => {}
            Offer::Held => {
                held = true;
                interrupted = true;
            }
            Offer::Refused => {
                interrupted = true;
                if actor.external_session_id.is_empty() {
                    match source {
                        DispatchSource::New(_) => gaps::record(group, actor, Some(event)),
                        DispatchSource::Unread(_) | DispatchSource::Directed => {
                            gaps::record(group, actor, None);
                        }
                    }
                }
            }
        }
    }
    DispatchOutcome {
        report: report(targets.len(), online, queued),
        interrupted,
        held,
    }
}

/// Process-owned jobs have passed the durable claim check for this attempt.
/// The previous daemon's live claims are reconciled by recovery outside this queue.
fn offer_delivery(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    transport: &'static str,
    claimed: bool,
    slot: Option<DeliverySlot>,
) -> Offer {
    let slot = match slot {
        Some(slot) => Ok(slot),
        None => DeliverySlot::reserve(&group.group_id, &actor.id, &event.id),
    };
    let offered = match &slot {
        Ok(_) => enqueue(DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed,
            transport,
        }),
        Err(offer) => *offer,
    };
    if offered == Offer::Queued
        && let Ok(slot) = slot
    {
        slot.transfer();
        return offered;
    }
    if offered == Offer::Refused {
        if claimed
            && let Err(error) = crate::ops::runtime_delivery::append_state_with_owner(
                home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                &event.id,
                transport,
                crate::ops::runtime_delivery::DeliveryOutcome::Unsubmitted(
                    "daemon delivery queue did not accept the event",
                ),
                crate::ops::runtime_delivery::OwnerAcknowledgement::NotOffered,
            )
        {
            tracing::warn!(message = %error.message, "failed to record rejected runtime delivery");
        }
        if !actor.external_session_id.is_empty() {
            set_refill(&group.group_id, &actor.id, true);
        }
    }
    offered
}

pub fn dispatch_unread(home: &HomeLayout, group: &GroupDoc, actor_id: &str) -> usize {
    if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
        set_refill(&group.group_id, actor_id, false);
        return 0;
    }
    let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
        set_refill(&group.group_id, actor_id, false);
        return 0;
    };
    if !actor.enabled || held_for_switch(group, actor) {
        set_refill(&group.group_id, actor_id, false);
        return 0;
    }
    let gap = gaps::resume_member(&group.group_id, actor_id);
    // Pull members need no pending-page read unless a running push owner takes their work.
    let saved_push = configured_for_push(actor);
    let observed =
        (!saved_push).then(|| crate::ops::actor_runtime::delivery_owner(&group.group_id, actor_id));
    if let Some(Ok(owner)) = &observed
        && !owner
            .as_ref()
            .is_some_and(|o| o.state == crate::ops::actor_runtime::OwnerState::Running)
    {
        set_refill(&group.group_id, actor_id, false);
        gaps::complete(&group.group_id, actor_id, gap.as_ref());
        return 0;
    }
    if let Some(Err(error)) = &observed {
        tracing::warn!(%error, group_id = %group.group_id, %actor_id,
            "failed to read the runtime owner for pending deliveries");
        record_member_catchup(group, actor);
        return 0;
    }
    let pause = if actor.external_session_id.is_empty() {
        crate::ops::actor_runtime::confirmation_pause_applies(home, &group.group_id, actor)
    } else {
        Ok(false)
    };
    match pause {
        Ok(false) => {}
        Ok(true) => {
            set_refill(&group.group_id, actor_id, false);
            gaps::complete(&group.group_id, actor_id, gap.as_ref());
            return 0;
        }
        Err(error) => {
            tracing::warn!(
                message = %error.message,
                group_id = %group.group_id,
                %actor_id,
                "failed to check confirmation pause for pending deliveries"
            );
            record_member_catchup(group, actor);
            return 0;
        }
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
            record_member_catchup(group, actor);
            return 0;
        }
    };
    if std::env::var_os("CCCC_DELIVERY_QUERY_PROBE").is_some_and(|value| value == "1") {
        let actor_index = group
            .actors
            .iter()
            .filter(|actor| actor.enabled)
            .position(|actor| actor.id == actor_id);
        cccc_runtime::diagnostic(format_args!(
            "CCCC_DELIVERY_QUERY {}",
            serde_json::json!({"group_id":group.group_id,"actor_index":actor_index,"selected":events.len()})
        ));
    }
    let native = !actor.external_session_id.is_empty();

    let full_page = events.len() == QUEUE_CAPACITY;
    if native {
        // Only a successful read consumes the previous refill request. A full
        // enqueue below can request another pass even for a partial page.
        set_refill(&group.group_id, actor_id, false);
    }
    if events.is_empty() {
        gaps::complete(&group.group_id, actor_id, gap.as_ref());
        return 0;
    }
    let transport = if native {
        "local_session"
    } else {
        let owner = observed.unwrap_or_else(|| {
            crate::ops::actor_runtime::delivery_owner(&group.group_id, actor_id)
        });
        let owner = match owner {
            Ok(owner) => owner,
            Err(error) => {
                tracing::warn!(%error, group_id = %group.group_id, %actor_id,
                    "failed to read the runtime owner for pending deliveries");
                gaps::record(group, actor, None);
                return 0;
            }
        };
        owner_transport(home, group, actor, owner.as_ref())
    };
    let mut queued = 0;
    let mut interrupted = false;
    let mut held = false;
    for event in events {
        let outcome = dispatch_to_inner_outcome(
            home,
            group,
            &event,
            std::slice::from_ref(actor),
            false,
            None,
            DispatchSource::Unread(transport),
        );
        queued += outcome.report.queued;
        if outcome.held {
            held = true;
            break;
        }
        if outcome.interrupted {
            interrupted = true;
            break;
        }
    }
    if held {
        return queued;
    }
    if !full_page && !interrupted {
        gaps::complete(&group.group_id, actor_id, gap.as_ref());
    }
    if full_page {
        // The whole page may already be in flight (or filled the queue). Its
        // tail still needs a later ledger read after those deliveries settle.
        record_member_catchup(group, actor);
    }
    queued
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

/// The channel that would take this Actor's work now. A failed owner query is
/// an error: claiming on a guessed channel records the wrong one.
pub(super) fn delivery_transport(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> std::io::Result<&'static str> {
    if !actor.external_session_id.is_empty() {
        return Ok("local_session");
    }
    let owner = crate::ops::actor_runtime::delivery_owner(&group.group_id, &actor.id)?;
    Ok(owner_transport(home, group, actor, owner.as_ref()))
}

/// The channel that takes an Actor's work: what runs, else what is saved.
pub(super) fn owner_transport(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    owner: Option<&crate::ops::actor_runtime::RegisteredOwner>,
) -> &'static str {
    use crate::ops::actor_runtime::OwnerIdentity;
    match owner.map(|owner| &owner.identity) {
        Some(OwnerIdentity::DeepSeek { .. }) => "deepseek",
        Some(OwnerIdentity::Managed { .. }) => "managed_session",
        Some(OwnerIdentity::Pty { .. }) => "pty",
        None => saved_transport(home, group, actor),
    }
}

fn saved_transport(home: &HomeLayout, group: &GroupDoc, actor: &Actor) -> &'static str {
    if actor.runtime == ActorRuntime::Deepseek {
        "deepseek"
    } else if actor.runtime == ActorRuntime::ChatgptDots {
        "web_model_pull"
    } else if actor.runtime.is_web_model() {
        web_model_delivery_transport(home, group, actor)
    } else if crate::ops::local_headless::uses_managed_delivery(&group.group_id, actor) {
        "managed_session"
    } else {
        "pty"
    }
}

fn web_model_delivery_transport(
    home: &HomeLayout,
    _group: &GroupDoc,
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
    if actor.runtime == ActorRuntime::GrokWebModel {
        return "web_model_pull";
    }

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
                connector["revoked"] != true && connector["provider"] == "chatgpt_web"
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

fn set_refill(group_id: &str, actor_id: &str, needed: bool) {
    if let Ok(mut pending) = refills().lock() {
        let key = (group_id.to_owned(), actor_id.to_owned());
        if needed {
            pending.insert(key, false);
        } else {
            pending.remove(&key);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Offer {
    Queued,
    /// The member's runtime is being switched. A claimed job was already
    /// given back as unsubmitted, so the switch's own dispatch offers it again.
    Held,
    /// The event already holds an in-flight slot in the worker or completion queue.
    AlreadyInFlight,
    Refused,
}

fn enqueue(job: DeliveryJob) -> Offer {
    let key = (job.group.group_id.clone(), job.actor.id.clone());
    let Ok(mut map) = workers().lock() else {
        return Offer::Refused;
    };
    if is_fenced(&key.0, &key.1) {
        // Settled under the worker map: the fence is lifted only afterwards.
        if job.claimed
            && let Err(error) = crate::ops::runtime_delivery::append_state_with_owner(
                &job.home,
                &key.0,
                &key.1,
                &job.actor.created_at,
                &job.event.id,
                job.transport,
                crate::ops::runtime_delivery::DeliveryOutcome::Unsubmitted(
                    "held while the member's runtime was switched",
                ),
                crate::ops::runtime_delivery::OwnerAcknowledgement::NotOffered,
            )
        {
            tracing::warn!(message = %error.message, "failed to give back held runtime delivery");
        }
        return Offer::Held;
    }
    let worker = map.entry(key.clone()).or_insert_with(|| spawn_worker(&key));
    let Some(sender) = worker.sender.as_ref() else {
        return Offer::Refused;
    };
    match sender.try_send(job) {
        Ok(()) => Offer::Queued,
        Err(TrySendError::Full(_)) => {
            tracing::warn!(group_id = %key.0, actor_id = %key.1, "PTY delivery queue is full");
            Offer::Refused
        }
        Err(TrySendError::Disconnected(job)) => {
            let replacement = spawn_worker(&key);
            let result = replacement
                .sender
                .as_ref()
                .is_some_and(|sender| sender.try_send(job).is_ok());
            let stale = map.insert(key, replacement);
            drop(map);
            if let Some(stale) = stale {
                stale.shutdown();
            }
            if result {
                Offer::Queued
            } else {
                Offer::Refused
            }
        }
    }
}

fn spawn_worker(key: &Key) -> DeliveryWorker {
    let (sender, receiver) = mpsc::sync_channel::<DeliveryJob>(QUEUE_CAPACITY);
    let name = format!("cccc-delivery:{}:{}", key.0, key.1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    let replaced = Arc::new(AtomicBool::new(false));
    let thread_replaced = Arc::clone(&replaced);
    let thread = std::thread::Builder::new().name(name).spawn(move || {
        let mut preamble_session = String::new();
        // Deferred work whose submission may have started. Only a runtime
        // replacement reads it: such work is never offered to the new owner.
        let mut attempted = HashSet::<String>::new();
        let mut deferred = Vec::new();
        let mut deferred_failures: u32 = 0;
        // Failed turns of the job at the head of the batch, by event id.
        let mut failed_turns = (String::new(), 0_u32);
        let mut wakes = actor_delivery_worker::WakeBudget::default();
        while !thread_cancelled.load(Ordering::Acquire) {
            let mut batch = if deferred.is_empty() {
                let Ok(job) = receiver.recv() else {
                    break;
                };
                vec![job]
            } else if !attempted.is_empty() || deferred.len() >= BATCH_CAPACITY {
                // Failed startup or paused delivery must not grow this batch
                // beyond its limit by draining the bounded channel on retries.
                // An admitted batch keeps its original event set until settled.
                if !actor_delivery_worker::interruptible_sleep(
                    deferred_retry_delay(deferred_failures),
                    &thread_cancelled,
                ) {
                    break;
                }
                std::mem::take(&mut deferred)
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
            if attempted.is_empty() && actor_delivery_worker::merges_batches(&batch[0]) {
                while batch.len() < BATCH_CAPACITY {
                    match receiver.try_recv() {
                        Ok(job) => batch.push(job),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            let mut delivered = false;
            let mut terminal_reason = None;
            let mut rejected = None;
            for attempt in 0..3 {
                match actor_delivery_worker::process_batch_tracked(
                    &mut batch,
                    &mut preamble_session,
                    &thread_cancelled,
                    &mut attempted,
                    &mut wakes,
                ) {
                    actor_delivery_worker::BatchOutcome::Delivered => {
                        delivered = true;
                        break;
                    }
                    actor_delivery_worker::BatchOutcome::Terminal(reason) => {
                        terminal_reason = Some(reason);
                        break;
                    }
                    // The provider already ran and failed this turn: try again
                    // only after the deferred backoff, never at once.
                    actor_delivery_worker::BatchOutcome::Rejected(reason) => {
                        rejected = Some(reason);
                        break;
                    }
                    actor_delivery_worker::BatchOutcome::Retry => {}
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
            if let Some(reason) = terminal_reason {
                close_jobs(&batch, &attempted, &reason);
                deferred_failures = 0;
                // Work queued behind it takes its turn with the starts already
                // spent: it would meet the same failure, once per message.
                match receiver.try_recv() {
                    Ok(job) => deferred.push(job),
                    Err(_) => wakes = Default::default(),
                }
            } else if let Some(reason) = rejected {
                let head = batch.first().map(|job| job.event.id.clone());
                if head.as_ref() != Some(&failed_turns.0) {
                    failed_turns = (head.unwrap_or_default(), 0);
                }
                failed_turns.1 += 1;
                if failed_turns.1 > FAILED_TURN_RETRIES && !batch.is_empty() {
                    let job = batch.remove(0);
                    exhaust_job(&job, &reason);
                    tell_sender(&job, false);
                    failed_turns = (String::new(), 0);
                } else if let Some(job) = batch.first_mut() {
                    // A definitive native failure settles this attempt before
                    // another claim can authorize the existing bounded retry.
                    // Keep the in-flight worker slot throughout the backoff.
                    match append_job_state(
                        job,
                        crate::ops::runtime_delivery::DeliveryOutcome::Failed(&reason),
                    ) {
                        Ok(_) => {
                            job.claimed = false;
                            attempted.remove(&job.event.id);
                        }
                        Err(error) => tracing::warn!(message = %error.message, "failed native outcome still awaits settlement"),
                    }
                }
                deferred = batch;
                deferred_failures = deferred_failures.saturating_add(1);
            } else if !delivered {
                deferred = batch;
                deferred_failures = deferred_failures.saturating_add(1);
            } else {
                deferred_failures = 0;
                wakes = Default::default();
            }
            attempted.retain(|id| deferred.iter().any(|job: &DeliveryJob| &job.event.id == id));
        }
        let replacing = thread_replaced.load(Ordering::Acquire);
        for job in deferred.into_iter().chain(receiver.try_iter()) {
            use crate::ops::runtime_delivery::OwnerAcknowledgement;
            let reason = "delivery worker stopped before runtime acceptance".to_owned();
            if !job.claimed {
                release_in_flight(&job);
                continue;
            }
            if crate::session_owner::enabled(&job.home)
                && crate::session_owner::maintenance_selected(&job.home)
                && job.actor.external_session_id.is_empty()
            {
                if !attempted.contains(&job.event.id) {
                    finish_job_with_owner(
                        &job,
                        CompletedDelivery::Unsubmitted(reason),
                        OwnerAcknowledgement::NotOffered,
                    );
                } else {
                    // The execution task belongs to its owner. The next
                    // controller queries it before settling this durable claim.
                    release_in_flight(&job);
                }
                continue;
            }
            // Each native handoff marks its own attempted input. A stopped
            // whole-turn transport may have received it before any acceptance
            // result; calling it failed would authorize an automatic replay.
            let native = !job.actor.external_session_id.is_empty();
            let (outcome, acknowledgement) = if attempted.contains(&job.event.id) {
                (
                    CompletedDelivery::Ambiguous(
                        if replacing { "runtime replaced while this delivery was being submitted" }
                        else { "runtime stopped after submission before its outcome was confirmed" }.into(),
                    ),
                    OwnerAcknowledgement::AfterWritePermit { known_only: true },
                )
            } else if replacing || native {
                (CompletedDelivery::Unsubmitted(reason), OwnerAcknowledgement::NotOffered)
            } else {
                (CompletedDelivery::Failed(reason), OwnerAcknowledgement::NotOffered)
            };
            finish_job_with_owner(&job, outcome, acknowledgement);
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
        replaced,
        thread,
    }
}

pub(super) fn deferred_retry_delay(failures: u32) -> std::time::Duration {
    let exponent = failures.saturating_sub(1).min(4);
    let delay = std::time::Duration::from_millis(250 * (1_u64 << exponent));
    delay.min(DEFERRED_RETRY_MAX)
}

#[cfg(test)]
#[path = "actor_delivery_tests.rs"]
mod tests;

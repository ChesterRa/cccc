use cccc_contracts::{Actor, ActorRuntime, GroupState};
use cccc_core::GroupStore;
use cccc_runtime::PtyHandoff;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ops::actor_delivery::{CompletedDelivery, DeliveryJob, complete_job, finish_job};
use crate::ops::actor_runtime;
use crate::ops::deepseek_runtime::TurnDelivery;

const PREAMBLE_DELAY: Duration = Duration::from_millis(500);
const INPUT_MODE_TIMEOUT: Duration = Duration::from_secs(5);
const ANTIGRAVITY_STARTUP_SETTLE: Duration = Duration::from_millis(1_500);
/// Automatic starts one batch may use. A program that cannot stay up would
/// otherwise restart for as long as its work waits, with nothing to show why.
/// The work stays recoverable: Start, Retry or a new message tries again.
const WAKES_PER_BATCH: u32 = 6;

#[cfg(all(test, unix))]
#[path = "actor_delivery_startup_tests.rs"]
mod startup_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOutcome {
    Delivered,
    Retry,
    /// The job can never succeed in the current configuration (e.g. the group
    /// has no attached scope); the delivery must be settled Failed instead of
    /// retried.
    Terminal(String),
    /// The runtime ran a turn for the first job and the provider reported it
    /// failed. The caller bounds how many such turns one job may cost.
    Rejected(String),
}

/// Scope-resolution errors match `actor_new_session`: without an attached scope
/// no runtime can ever start, so the delivery must fail fast rather than loop
/// claimed/stalled.
fn terminal_reason(error: &crate::dispatch::OpError) -> Option<String> {
    matches!(
        error.code.as_str(),
        "missing_project_root" | "scope_not_attached" | "invalid_project_root"
    )
    .then(|| error.message.clone())
}

/// Deliver `jobs`. Jobs settled here for good (stale Mail notices) are removed
/// from `jobs`, so a caller that retries the batch never offers them again.
#[cfg(test)]
pub fn process_batch(
    jobs: &mut Vec<DeliveryJob>,
    preamble_session: &mut String,
    cancelled: &AtomicBool,
) -> BatchOutcome {
    process_batch_tracked(
        jobs,
        preamble_session,
        cancelled,
        &mut HashSet::new(),
        &mut WakeBudget::default(),
    )
}

/// As [`process_batch`], adding to `attempted` the event ids of managed work
/// right before its submission can begin: a PTY payload write, a DeepSeek
/// request, or a managed session's input. A runtime replacement records work
/// that stays unconfirmed as ambiguous instead of offering it again. `wakes`
/// holds the automatic starts this batch has used, across retries.
pub fn process_batch_tracked(
    jobs: &mut Vec<DeliveryJob>,
    preamble_session: &mut String,
    cancelled: &AtomicBool,
    attempted: &mut HashSet<String>,
    wakes: &mut WakeBudget,
) -> BatchOutcome {
    let Some(job) = jobs.first().cloned() else {
        return BatchOutcome::Retry;
    };
    if cancelled.load(Ordering::Acquire) {
        return BatchOutcome::Retry;
    }
    let Ok(current_group) =
        GroupStore::new(job.home.clone()).and_then(|store| store.load(&job.group.group_id))
    else {
        return BatchOutcome::Retry;
    };
    if matches!(
        current_group.state,
        GroupState::Paused | GroupState::Stopped
    ) {
        return BatchOutcome::Retry;
    }
    let Some(current_actor) = current_group
        .actors
        .iter()
        .find(|actor| actor.id == job.actor.id)
        .cloned()
    else {
        return BatchOutcome::Retry;
    };
    if !current_actor.enabled {
        return BatchOutcome::Retry;
    }
    // A job queued from an older snapshot can outlive a switch's fence. Until
    // the switch has written its handoff, nothing reaches the new owner: the
    // work goes back unsubmitted and follows the handoff once it is written.
    if cccc_core::runtime_switches::pending_for(&current_group, &current_actor).is_some() {
        for job in jobs.drain(..) {
            if job.claimed {
                finish_job(
                    &job,
                    CompletedDelivery::Unsubmitted(
                        "held until the member's runtime switch is completed".into(),
                    ),
                );
            } else {
                super::actor_delivery::release_in_flight(&job);
            }
        }
        return BatchOutcome::Delivered;
    }
    // Every attempt, including retries, re-checks Mail notices against their
    // Mail: one read, answered, or delivered since is withdrawn, never sent, and
    // leaves the batch for good.
    withdraw_stale_mail_notices(jobs, &job.home, &current_group, &current_actor);
    if jobs.is_empty() {
        return BatchOutcome::Delivered;
    }
    if !current_actor.external_session_id.is_empty() {
        return process_external_batch(
            jobs,
            &current_group,
            &current_actor,
            preamble_session,
            cancelled,
        );
    }
    // What runs now takes the work, whatever was saved for the next launch.
    let owner = match actor_runtime::delivery_owner(&current_group.group_id, &current_actor.id) {
        Ok(owner) => owner,
        Err(error) => {
            tracing::warn!(
                %error,
                group_id = %current_group.group_id,
                actor_id = %current_actor.id,
                "failed to read the Actor's runtime owner"
            );
            return BatchOutcome::Retry;
        }
    };
    // A claim names the channel it was made for. When that is no longer what
    // takes the work, the work goes back for the current owner to claim.
    let transport = super::actor_delivery::owner_transport(
        &job.home,
        &current_group,
        &current_actor,
        owner.as_ref(),
    );
    if jobs.iter().any(|job| job.transport != transport) {
        return_to_queue(jobs, attempted);
        super::actor_delivery::dispatch_unread(&job.home, &current_group, &current_actor.id);
        return BatchOutcome::Delivered;
    }
    if transport == "deepseek" {
        return process_deepseek_batch(
            jobs,
            &job.home,
            &current_group,
            &current_actor,
            cancelled,
            attempted,
            wakes,
        );
    }
    let jobs = jobs.as_slice();
    if crate::ops::local_headless::uses_managed_delivery(&current_group.group_id, &current_actor) {
        return process_managed_batch(
            jobs,
            &job.home,
            &current_group,
            &current_actor,
            cancelled,
            attempted,
            wakes,
        );
    }
    let status = match ensure_running(&job.home, &current_group, &current_actor, wakes) {
        Ok(Some(status)) => status,
        Ok(None) => return BatchOutcome::Retry,
        Err(reason) => return BatchOutcome::Terminal(reason),
    };
    // Input follows the terminal that runs. It may predate the saved settings.
    let current_actor = status.launch.map_or(current_actor.clone(), |launch| {
        actor_runtime::as_launched(&current_actor, launch)
    });
    let first_delivery = *preamble_session != status.started_at;
    if first_delivery {
        if current_actor.runtime != ActorRuntime::Custom
            && !wait_for_input_mode(&current_group.group_id, &current_actor.id, cancelled)
        {
            return BatchOutcome::Retry;
        }
        // agy can enable paste mode before its conversation input is mounted.
        // The ordinary submit delay comes AFTER writing and cannot protect the
        // first payload. Allow this observed startup transition to settle before
        // writing anything. This is bounded PTY pacing, not a provider handshake.
        if current_actor.runtime == ActorRuntime::Antigravity
            && !interruptible_sleep(ANTIGRAVITY_STARTUP_SETTLE, cancelled)
        {
            return BatchOutcome::Retry;
        }
        // Custom terminal programs retain their line-oriented preamble contract.
        // Native agents receive their startup context and task in one submission.
        if current_actor.runtime == ActorRuntime::Custom {
            if !matches!(
                submit_text(
                    &current_group.group_id,
                    &current_actor,
                    &status,
                    &super::actor_delivery_preamble::render(
                        &job.home,
                        &current_group,
                        &current_actor
                    ),
                    cancelled,
                ),
                PtyHandoff::Accepted
            ) {
                return BatchOutcome::Retry;
            }
            preamble_session.clone_from(&status.started_at);
            if !interruptible_sleep(PREAMBLE_DELAY, cancelled) {
                return BatchOutcome::Retry;
            }
        }
    }

    let events = jobs.iter().map(|job| job.event.clone()).collect::<Vec<_>>();
    let Some(mut payload) = super::actor_delivery_render::render_batch_with_mail_context(
        &job.home,
        &current_group,
        &current_actor.id,
        &events,
    ) else {
        return BatchOutcome::Retry;
    };
    if first_delivery && current_actor.runtime != ActorRuntime::Custom {
        payload = format!(
            "{}\n\n{payload}",
            super::actor_delivery_preamble::render(&job.home, &current_group, &current_actor)
                .trim_end()
        );
    } else if current_actor.runtime == ActorRuntime::Antigravity {
        payload = format!(
            "[CCCC] If this conversation has not completed CCCC initialization, call cccc_bootstrap before handling this task. Otherwise continue without repeating bootstrap.\n\n{payload}"
        );
    }
    let instructions = super::instruction_files::check(
        &job.home,
        &current_group.group_id,
        &current_actor,
        &actor_runtime::pty_instruction_session(&status),
        &status.cwd,
    );
    if let Some(pending) = &instructions {
        payload = format!("{payload}\n\n{}", pending.delivery_line());
    }
    attempted.extend(jobs.iter().map(|job| job.event.id.clone()));
    match submit_text(
        &current_group.group_id,
        &current_actor,
        &status,
        &payload,
        cancelled,
    ) {
        PtyHandoff::Accepted => {
            if let Some(pending) = instructions {
                pending.delivered();
            }
            preamble_session.clone_from(&status.started_at);
            finish_jobs(jobs);
            BatchOutcome::Delivered
        }
        PtyHandoff::NotWritten(_) => {
            for job in jobs {
                attempted.remove(&job.event.id);
            }
            BatchOutcome::Retry
        }
        // Writing it again could leave the terminal with two copies.
        PtyHandoff::Uncertain(reason) => {
            for job in jobs {
                finish_job(
                    job,
                    CompletedDelivery::Ambiguous(format!(
                        "the terminal may hold part of this delivery: {reason}"
                    )),
                );
            }
            BatchOutcome::Delivered
        }
    }
}

/// The owner that took this work is gone. The next owner claims what was never
/// written; a handoff that may have been written stays for review, like any other
/// replaced owner's.
fn return_to_queue(jobs: &mut Vec<DeliveryJob>, attempted: &HashSet<String>) {
    for job in jobs.drain(..) {
        if attempted.contains(&job.event.id) {
            finish_job(
                &job,
                CompletedDelivery::Ambiguous(
                    "its owner stopped while this delivery was being submitted".into(),
                ),
            );
        } else {
            finish_job(
                &job,
                CompletedDelivery::Unsubmitted(
                    "its owner stopped before this delivery was submitted".into(),
                ),
            );
        }
    }
}

fn process_external_batch(
    jobs: &mut Vec<DeliveryJob>,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    preamble_session: &mut String,
    cancelled: &AtomicBool,
) -> BatchOutcome {
    use super::actor_delivery::{CompletedDelivery, finish_job};
    use super::local_sessions::DeliveryOutcome;
    while let Some(job) = jobs.first_mut() {
        if cancelled.load(Ordering::Acquire) {
            return BatchOutcome::Retry;
        }
        let Ok(group) =
            GroupStore::new(job.home.clone()).and_then(|store| store.load(&group.group_id))
        else {
            return BatchOutcome::Retry;
        };
        let Some(actor) = group.actors.iter().find(|current| {
            current.id == actor.id
                && cccc_core::actors::generation_identity(current)
                    == cccc_core::actors::generation_identity(&job.actor)
                && current.external_session_id == job.actor.external_session_id
                && current.enabled
        }) else {
            return BatchOutcome::Retry;
        };
        if matches!(group.state, GroupState::Paused | GroupState::Stopped) {
            return BatchOutcome::Retry;
        }
        let Some(session) =
            super::local_sessions::session_marker(&job.home, &actor.external_session_id)
        else {
            return defer_external_job(job);
        };
        if !super::local_sessions::connected(&job.home, &actor.external_session_id) {
            return defer_external_job(job);
        }
        let events = std::slice::from_ref(&job.event);
        let Some(mut text) = super::actor_delivery_render::render_batch_with_mail_context(
            &job.home, &group, &actor.id, events,
        ) else {
            return BatchOutcome::Terminal("Native delivery could not be rendered".into());
        };
        let preamble = super::actor_delivery_preamble::render(&job.home, &group, actor);
        let marker = preamble_marker(&session, chrono::Utc::now().timestamp(), &preamble);
        let first_delivery = *preamble_session != marker;
        let mut instructions = super::local_sessions::instruction_session(&job.home, actor)
            .and_then(|(session, root)| {
                super::instruction_files::check(&job.home, &group.group_id, actor, &session, &root)
            });
        if actor.runtime == ActorRuntime::Claude {
            let short = instructions
                .as_ref()
                .map(super::instruction_files::Pending::short_line);
            let Some((notice, included)) = super::actor_delivery_render::claude_notice(
                &group,
                actor,
                &job.event,
                short.as_deref(),
                &text,
                first_delivery,
            ) else {
                return BatchOutcome::Terminal(
                    "Native notification routing context exceeds the Claude Monitor budget".into(),
                );
            };
            if !included {
                instructions = None;
            }
            text = notice;
        } else if first_delivery {
            text = format!("{preamble}\n\n{text}");
        } else {
            // One native session may serve several Groups. Later messages
            // still identify their receiving member without repeating startup.
            let scope = cccc_core::group_scope::effective_scope(&group, Some(actor));
            let context = serde_json::json!({
                "group_id":group.group_id,
                "group_title":group.title,
                "actor_id":actor.id,
                "role":cccc_core::actors::effective_role(&group, &actor.id),
                "scope_key":scope.map(|scope| &scope.scope_key),
                // Shown in the same spelling as the startup prompt's workspace.
                "scope_root":scope.map(|scope| {
                    cccc_runtime::compatible_path(std::path::Path::new(&scope.url))
                }),
            });
            text = format!("[CCCC] Receiving context: {context}\n\n{text}");
        }
        if actor.runtime != ActorRuntime::Claude {
            if let Some(pending) = &instructions {
                let noticed = format!("{text}\n\n{}", pending.delivery_line());
                // The notice is never why a message cannot be delivered.
                if matches!(
                    super::local_sessions::preflight(&job.home, actor, &noticed),
                    Err(super::local_sessions::PreflightError::Rejected(_))
                ) {
                    instructions = None;
                } else {
                    text = noticed;
                }
            }
        }
        match super::local_sessions::preflight(&job.home, actor, &text) {
            Ok(()) => {}
            Err(super::local_sessions::PreflightError::Unavailable) => {
                return defer_external_job(job);
            }
            Err(super::local_sessions::PreflightError::Rejected(reason)) => {
                // No external I/O is possible for this payload. Settle before
                // claiming, but do not replace another attempt's durable claim
                // or terminal evidence with a newly rendered preflight failure.
                if !job.claimed {
                    match super::runtime_delivery::latest_state(
                        &job.home,
                        &group.group_id,
                        &actor.id,
                        &job.event.id,
                    ) {
                        Ok(Some((state, _)))
                            if matches!(
                                state.as_str(),
                                "claimed" | "accepted" | "ambiguous" | "withdrawn"
                            ) =>
                        {
                            super::actor_delivery::release_in_flight(job);
                            jobs.remove(0);
                            continue;
                        }
                        Err(_) => return BatchOutcome::Retry,
                        _ => {}
                    }
                }
                finish_job(job, CompletedDelivery::Failed(reason));
                jobs.remove(0);
                continue;
            }
        }
        if !job.claimed {
            match super::runtime_delivery::claim(
                &job.home,
                &group,
                actor,
                &job.event.id,
                "local_session",
                false,
            ) {
                Ok(super::runtime_delivery::ClaimResult::Claimed) => job.claimed = true,
                Ok(super::runtime_delivery::ClaimResult::Terminal(_)) => {
                    super::actor_delivery::release_in_flight(job);
                    jobs.remove(0);
                    continue;
                }
                Err(error) => {
                    tracing::warn!(message = %error.message, "failed to claim native handoff");
                    return BatchOutcome::Retry;
                }
            }
        }
        match super::local_sessions::submit(&job.home, &group, actor, events, text, cancelled) {
            DeliveryOutcome::Unavailable => return defer_external_job(job),
            DeliveryOutcome::Accepted => {
                finish_job(job, CompletedDelivery::Accepted);
                *preamble_session = marker;
                if let Some(pending) = instructions {
                    pending.delivered();
                }
            }
            DeliveryOutcome::Rejected(reason) => finish_job(job, CompletedDelivery::Failed(reason)),
            DeliveryOutcome::Ambiguous(reason) => {
                finish_job(job, CompletedDelivery::Ambiguous(reason))
            }
        }
        // A rejected or uncertain handoff never holds the remaining jobs hostage.
        jobs.remove(0);
    }
    BatchOutcome::Delivered
}

/// A delivery owes the Preamble when this differs from the last one accepted: in
/// a new conversation, after a reported compaction and when its text changes.
/// The hour gives a compaction nobody reported one more chance, since a missed
/// signal looks just like none.
fn preamble_marker(session: &str, now: i64, preamble: &str) -> String {
    format!("{session}:{}\n{preamble}", now / 3_600)
}

fn defer_external_job(job: &mut DeliveryJob) -> BatchOutcome {
    if job.claimed {
        // Unavailable proves this attempt never crossed the native handoff
        // boundary. Keep the in-memory slot, but not an uncertain durable claim.
        match super::runtime_delivery::append_state(
            &job.home,
            &job.group.group_id,
            &job.actor.id,
            &job.actor.created_at,
            &job.event.id,
            "local_session",
            super::runtime_delivery::DeliveryOutcome::Unsubmitted(
                "Native receive channel unavailable before submission",
            ),
        ) {
            Ok(_) => job.claimed = false,
            Err(error) => {
                tracing::warn!(message = %error.message, "failed to release unsubmitted native claim");
            }
        }
    }
    BatchOutcome::Retry
}

/// Delivered jobs leave `jobs`, so a rejected batch always starts with the job
/// whose turn failed.
fn process_deepseek_batch(
    jobs: &mut Vec<DeliveryJob>,
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    cancelled: &AtomicBool,
    attempted: &mut HashSet<String>,
    wakes: &mut WakeBudget,
) -> BatchOutcome {
    if !crate::ops::deepseek_runtime::running(&group.group_id, &actor.id) {
        if crate::ops::deepseek_runtime::manual_restart_required(home, group, actor) {
            return BatchOutcome::Retry;
        }
        match wakes.spend(&group.group_id, &actor.id) {
            Ok(true) => {}
            Ok(false) => return BatchOutcome::Retry,
            Err(reason) => return BatchOutcome::Terminal(reason),
        }
        match actor_runtime::apply(home, group, &actor.id, "actor.start") {
            Ok(_) if crate::ops::deepseek_runtime::running(&group.group_id, &actor.id) => {}
            Ok(_) => return BatchOutcome::Retry,
            Err(error) => {
                return match terminal_reason(&error) {
                    Some(reason) => BatchOutcome::Terminal(reason),
                    None => BatchOutcome::Retry,
                };
            }
        }
    }
    while let Some(job) = jobs.first() {
        if cancelled.load(Ordering::Acquire) {
            return BatchOutcome::Retry;
        }
        let started = AtomicBool::new(false);
        let delivery = crate::ops::deepseek_runtime::deliver_marked(
            home, group, actor, &job.event, cancelled, &started,
        );
        if started.load(Ordering::Acquire) {
            attempted.insert(job.event.id.clone());
        }
        match delivery {
            TurnDelivery::Completed => complete_job(&jobs.remove(0)),
            TurnDelivery::Failed(reason) => return BatchOutcome::Rejected(reason),
            TurnDelivery::Incomplete => return BatchOutcome::Retry,
        }
    }
    BatchOutcome::Delivered
}

fn process_managed_batch(
    jobs: &[DeliveryJob],
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    cancelled: &AtomicBool,
    attempted: &mut HashSet<String>,
    wakes: &mut WakeBudget,
) -> BatchOutcome {
    if !crate::ops::local_headless::running(&group.group_id, &actor.id) {
        match wakes.spend(&group.group_id, &actor.id) {
            Ok(true) => {}
            Ok(false) => return BatchOutcome::Retry,
            Err(reason) => return BatchOutcome::Terminal(reason),
        }
        match actor_runtime::apply(home, group, &actor.id, "actor.start") {
            Ok(None) if crate::ops::local_headless::running(&group.group_id, &actor.id) => {}
            Ok(_) => return BatchOutcome::Retry,
            Err(error) => {
                if let Some(reason) = terminal_reason(&error) {
                    return BatchOutcome::Terminal(reason);
                }
                if error.code == actor_runtime::RUNTIME_CONFIRMATION_PAUSED {
                    // No native submission occurred. Keep the durable messages
                    // retryable while freeing the worker for an explicit start.
                    for job in jobs {
                        finish_job(job, CompletedDelivery::Unsubmitted(error.message.clone()));
                    }
                    return BatchOutcome::Delivered;
                }
                if matches!(
                    error.code.as_str(),
                    actor_runtime::CLAUDE_RESUME_FAILED | actor_runtime::RUNTIME_RESUME_FAILED
                ) {
                    // Release this worker's claims but leave inbox/ledger messages pending.
                    // Explicit recovery redispatches them; automatic startup must stop.
                    super::actor_delivery::fail_jobs(jobs, &error.message);
                    return BatchOutcome::Delivered;
                }
                tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %error.message,
                    "failed to auto-wake managed actor for message delivery"
                );
                return BatchOutcome::Retry;
            }
        }
    }
    let events = jobs.iter().map(|job| job.event.clone()).collect::<Vec<_>>();
    let started = AtomicBool::new(false);
    let outcome = crate::ops::local_headless::submit_batch_marked(
        home, group, actor, &events, cancelled, &started,
    );
    if started.load(Ordering::Acquire) {
        attempted.extend(jobs.iter().map(|job| job.event.id.clone()));
    }
    settle_managed_batch(jobs, home, group, actor, outcome)
}

/// Withdraw Mail notices none of whose Mail still awaits the actor and remove
/// them from `jobs`; the remaining jobs are left to deliver.
fn withdraw_stale_mail_notices(
    jobs: &mut Vec<DeliveryJob>,
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
) {
    let (stale, live): (Vec<_>, Vec<_>) = std::mem::take(jobs).into_iter().partition(|job| {
        crate::ops::runtime_delivery::is_stale_mail_notice(home, group, &actor.id, &job.event)
    });
    *jobs = live;
    if !stale.is_empty() {
        super::actor_delivery::withdraw_jobs(
            &stale,
            crate::ops::runtime_delivery::STALE_MAIL_NOTICE,
        );
    }
}

/// Record a managed submission's outcome. Returns Retry only for Deferred,
/// the one outcome that enters automatic retry.
fn settle_managed_batch(
    jobs: &[DeliveryJob],
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    outcome: crate::ops::local_headless::BatchSubmission,
) -> BatchOutcome {
    match outcome {
        crate::ops::local_headless::BatchSubmission::Accepted => finish_jobs(jobs),
        crate::ops::local_headless::BatchSubmission::Deferred => return BatchOutcome::Retry,
        crate::ops::local_headless::BatchSubmission::Withheld => {
            super::actor_delivery::withdraw_jobs(
                jobs,
                "early Mail notice withheld while the Actor works; the unread tick re-issues it",
            );
        }
        crate::ops::local_headless::BatchSubmission::Unconfirmed => {
            for job in jobs {
                if let Err(error) = crate::ops::runtime_delivery::append_state(
                    home,
                    &group.group_id,
                    &actor.id,
                    &actor.created_at,
                    &job.event.id,
                    job.transport,
                    crate::ops::runtime_delivery::DeliveryOutcome::Ambiguous(
                        "ACP prompt receipt was not confirmed; inspect the Actor before an explicit retry",
                    ),
                ) {
                    // The original durable claim remains unretryable and is
                    // reconciled as ambiguous on daemon restart.
                    tracing::error!(message=%error.message, event_id=%job.event.id, "failed to persist unconfirmed ACP delivery");
                }
                super::actor_delivery::release_in_flight(job);
            }
        }
    }
    // Terminal handling includes uncertainty; only Deferred enters automatic retry.
    BatchOutcome::Delivered
}

/// Terminal programs other than Custom take a batch as one submission. Managed
/// and DeepSeek sessions take their messages one turn at a time.
pub(super) fn merges_batches(job: &DeliveryJob) -> bool {
    use actor_runtime::{OwnerIdentity, RegisteredOwner};
    let actor = match actor_runtime::delivery_owner(&job.group.group_id, &job.actor.id) {
        Ok(None) => job.actor.clone(),
        Ok(Some(RegisteredOwner {
            identity: OwnerIdentity::Pty { launch, .. },
            ..
        })) => actor_runtime::as_launched(&job.actor, launch),
        Ok(Some(_)) | Err(_) => return false,
    };
    actor.runtime != ActorRuntime::Custom
        && actor.runtime != ActorRuntime::Deepseek
        && !crate::ops::local_headless::supports(&actor)
}

fn finish_jobs(jobs: &[DeliveryJob]) {
    for job in jobs {
        complete_job(job);
    }
}

/// The automatic starts a batch has used. The spacing is real time: every new
/// message ends a deferred wait early, so a count alone would be spent at once
/// and a program that is only being replaced would never get its few seconds.
#[derive(Default)]
pub struct WakeBudget {
    spent: u32,
    next: Option<std::time::Instant>,
}

impl WakeBudget {
    /// Whether to start now. `Err` says why there will be no other start.
    fn spend(&mut self, group_id: &str, actor_id: &str) -> Result<bool, String> {
        if self.spent >= WAKES_PER_BATCH {
            return Err(stopped_reason(group_id, actor_id));
        }
        let now = std::time::Instant::now();
        if self.next.is_some_and(|next| now < next) {
            return Ok(false);
        }
        self.spent += 1;
        self.next = Some(now + super::actor_delivery::deferred_retry_delay(self.spent));
        Ok(true)
    }
}

/// Only the exit code: what the program printed stays in its terminal.
fn stopped_reason(group_id: &str, actor_id: &str) -> String {
    match cccc_runtime::status(group_id, actor_id)
        .ok()
        .and_then(|status| status.exit_code)
    {
        Some(code) => format!("The agent exited (code {code}); check its terminal, then retry"),
        None => "The agent could not be kept running; check its command and terminal, then retry"
            .to_owned(),
    }
}

fn ensure_running(
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    wakes: &mut WakeBudget,
) -> Result<Option<cccc_runtime::SessionStatus>, String> {
    if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
        && status.running
    {
        return Ok(Some(status));
    }
    if !wakes.spend(&group.group_id, &actor.id)? {
        return Ok(None);
    }
    let status = match actor_runtime::apply(home, group, &actor.id, "actor.start") {
        Ok(Some(status)) if status.running => status,
        Ok(_) => return Ok(None),
        Err(error) => {
            if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
                && status.running
            {
                status
            } else if let Some(reason) = terminal_reason(&error) {
                return Err(reason);
            } else {
                tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %error.message,
                    "failed to auto-wake actor for message delivery"
                );
                return Ok(None);
            }
        }
    };
    Ok(Some(status))
}

fn submit_text(
    group_id: &str,
    actor: &Actor,
    expected: &cccc_runtime::SessionStatus,
    text: &str,
    cancelled: &AtomicBool,
) -> PtyHandoff {
    // Configuration and terminal ownership can change while this batch is waiting.
    // Reclassify before each preamble/payload, then pin the observed PTY at the writer.
    if super::local_headless::uses_managed_delivery(group_id, actor) {
        return PtyHandoff::NotWritten(cccc_runtime::PtyHandoffReason::SessionChanged);
    }
    super::actor_delivery::submit_terminal_text_for_session(expected, actor, text, cancelled)
}

#[cfg(test)]
fn submit_sequence(actor: &Actor) -> &'static [&'static [u8]] {
    super::actor_delivery::terminal_submit_sequence(actor)
}

fn wait_for_input_mode(group_id: &str, actor_id: &str, cancelled: &AtomicBool) -> bool {
    let deadline = std::time::Instant::now() + INPUT_MODE_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if cccc_runtime::bracketed_paste_enabled(group_id, actor_id).unwrap_or(false) {
            return true;
        }
        if !cccc_runtime::status(group_id, actor_id).is_ok_and(|status| status.running) {
            return false;
        }
        if !interruptible_sleep(Duration::from_millis(50), cancelled) {
            return false;
        }
    }
    !cancelled.load(Ordering::Acquire)
}

pub(super) fn interruptible_sleep(duration: Duration, cancelled: &AtomicBool) -> bool {
    let deadline = std::time::Instant::now().checked_add(duration);
    while !cancelled.load(Ordering::Acquire) {
        let remaining = deadline.map_or(Duration::from_millis(50), |deadline| {
            deadline.saturating_duration_since(std::time::Instant::now())
        });
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_new_message_cannot_hurry_the_next_start() {
        let mut wakes = WakeBudget::default();
        assert_eq!(wakes.spend("group", "peer1"), Ok(true));
        assert!(wakes.next.is_some());
        // Far enough that a paused test run cannot reach it.
        wakes.next = Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
        for _ in 0..100 {
            assert_eq!(wakes.spend("group", "peer1"), Ok(false));
        }
        assert_eq!(wakes.spent, 1);
    }

    #[test]
    fn the_last_start_is_followed_by_the_reason() {
        let mut wakes = WakeBudget::default();
        for _ in 0..WAKES_PER_BATCH {
            wakes.next = None;
            assert_eq!(wakes.spend("group", "peer1"), Ok(true));
        }
        assert!(
            wakes
                .spend("group", "peer1")
                .is_err_and(|reason| reason.contains("then retry"))
        );
    }

    use super::*;
    use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, Event};

    #[test]
    fn the_preamble_comes_due_with_a_new_session_hour_or_text() {
        let marker = preamble_marker("revision:0", 7_200, "rules");
        assert_eq!(marker, preamble_marker("revision:0", 10_799, "rules"));
        assert_ne!(marker, preamble_marker("revision:0", 10_800, "rules"));
        assert_ne!(marker, preamble_marker("revision:1", 7_200, "rules"));
        assert_ne!(marker, preamble_marker("revision:0", 7_200, "new rules"));
    }

    #[test]
    fn repeats_enter_only_for_tuis_that_can_drop_the_first_submit() {
        let mut actor = Actor::new("peer1");
        actor.submit = ActorSubmit::Enter;

        actor.runtime = ActorRuntime::Codex;
        assert_eq!(
            submit_sequence(&actor),
            &[b"\r".as_slice(), b"\r".as_slice()]
        );

        actor.runtime = ActorRuntime::Copilot;
        assert_eq!(
            submit_sequence(&actor),
            &[b"\r".as_slice(), b"\r".as_slice()]
        );

        actor.runtime = ActorRuntime::Claude;
        assert_eq!(submit_sequence(&actor), &[b"\r".as_slice()]);

        actor.submit = ActorSubmit::Newline;
        assert_eq!(submit_sequence(&actor), &[b"\n".as_slice()]);

        actor.submit = ActorSubmit::None;
        assert!(submit_sequence(&actor).is_empty());
    }

    #[test]
    fn a_retried_mail_notice_is_withdrawn_once_its_mail_was_read() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("stale notice", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.data = serde_json::json!({"to":["peer1"],"text":"review","message_mode":"mail"})
            .as_object()
            .cloned()
            .expect("mail");
        cccc_core::ledger::append(&ledger_path, &mail).expect("Mail");
        let mut notice = Event::new("system.notify", &group.group_id);
        notice.data = serde_json::json!({
            "kind":"mail_notice","target_actor_id":"peer1",
            "context":{"actor_id":"peer1","source_event_ids":[mail.id]}
        })
        .as_object()
        .cloned()
        .expect("notice");
        cccc_core::ledger::append(&ledger_path, &notice).expect("notice");
        crate::ops::runtime_delivery::claim(&home, &group, &actor, &notice.id, "pty", false)
            .expect("claim");
        let job = DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: notice.clone(),
            claimed: true,
            transport: "pty",
        };
        let state = || {
            crate::ops::runtime_delivery::latest_state(&home, &group.group_id, "peer1", &notice.id)
                .expect("state")
                .map(|(state, _)| state)
        };

        // The first attempt was deferred; the Mail is still unread, so it stays.
        let mut live = vec![job.clone()];
        withdraw_stale_mail_notices(&mut live, &home, &group, &actor);
        assert_eq!(live.len(), 1);
        assert_eq!(state().as_deref(), Some("claimed"));

        // The actor reads the Mail before the retry: the notice is withdrawn and
        // the retry finishes without starting or writing to the actor.
        cccc_core::inbox::consume_unread(&home, &group, "peer1", "peer1", 50).expect("read");
        let mut batch = vec![job.clone()];
        assert_eq!(
            process_batch(&mut batch, &mut String::new(), &AtomicBool::new(false)),
            BatchOutcome::Delivered
        );
        assert!(batch.is_empty(), "the withdrawn notice left the batch");
        assert_eq!(state().as_deref(), Some("withdrawn"));
        assert!(
            cccc_runtime::status(&group.group_id, "peer1").is_err(),
            "no runtime was started for a stale notice"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_withdrawn_notice_never_rejoins_a_batch_kept_for_retry() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("mixed batch", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        let event = |kind: &str, data: serde_json::Value| {
            let mut event = Event::new(kind, &group.group_id);
            event.by = "user".into();
            event.data = data.as_object().cloned().expect("data");
            cccc_core::ledger::append(&ledger_path, &event).expect("append");
            event
        };
        let mail = event(
            "chat.message",
            serde_json::json!({"to":["peer1"],"text":"review","message_mode":"mail"}),
        );
        let notice = event(
            "system.notify",
            serde_json::json!({"kind":"mail_notice","target_actor_id":"peer1",
                "context":{"source_event_ids":[mail.id]}}),
        );
        let send = event(
            "chat.message",
            serde_json::json!({"to":["peer1"],"text":"work","message_mode":"send"}),
        );
        cccc_core::inbox::consume_unread(&home, &group, "peer1", "peer1", 50).expect("read");
        let job = |event: &Event| DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed: true,
            transport: "pty",
        };
        let mut batch = vec![job(&notice), job(&send)];

        withdraw_stale_mail_notices(&mut batch, &home, &group, &actor);
        assert_eq!(
            batch
                .iter()
                .map(|job| job.event.id.clone())
                .collect::<Vec<_>>(),
            vec![send.id.clone()],
            "only the Send stays for a retry"
        );

        // The next attempt cannot check Mail (the ledger is unreadable); the
        // withdrawn notice is simply no longer part of the batch.
        let readable = std::fs::metadata(&ledger_path)
            .expect("ledger")
            .permissions();
        std::fs::set_permissions(&ledger_path, std::fs::Permissions::from_mode(0o000))
            .expect("unreadable ledger");
        withdraw_stale_mail_notices(&mut batch, &home, &group, &actor);
        std::fs::set_permissions(&ledger_path, readable).expect("restore ledger");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].event.id, send.id);
    }

    #[test]
    fn withheld_notices_are_withdrawn_instead_of_retried() {
        use crate::ops::local_headless::BatchSubmission;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("withheld delivery", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let notice = |kind: &str| {
            let mut event = Event::new("system.notify", &group.group_id);
            event.data = serde_json::json!({"kind":kind,"target_actor_id":"peer1"})
                .as_object()
                .cloned()
                .expect("notice");
            DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event,
                claimed: true,
                transport: "pty",
            }
        };
        let state = |job: &DeliveryJob| {
            crate::ops::runtime_delivery::latest_state(
                &home,
                &group.group_id,
                "peer1",
                &job.event.id,
            )
            .expect("state")
            .map(|(state, _)| state)
        };

        let held = notice("mail_notice");
        assert_eq!(
            settle_managed_batch(
                std::slice::from_ref(&held),
                &home,
                &group,
                &actor,
                BatchSubmission::Withheld
            ),
            BatchOutcome::Delivered,
            "a withheld notice leaves the retry lane"
        );
        assert_eq!(state(&held).as_deref(), Some("withdrawn"));

        let deferred = notice("mail_notice");
        assert_eq!(
            settle_managed_batch(
                std::slice::from_ref(&deferred),
                &home,
                &group,
                &actor,
                BatchSubmission::Deferred
            ),
            BatchOutcome::Retry,
            "Deferred still retries"
        );
        assert_eq!(state(&deferred), None);
    }

    #[test]
    fn disabled_actor_batch_does_not_start_or_change_its_lifecycle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("disabled delivery", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.enabled = false;
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = serde_json::json!({"to":["peer1"],"text":"do not wake"})
            .as_object()
            .cloned()
            .expect("event data");
        let job = DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event,
            claimed: true,
            transport: "pty",
        };

        assert_eq!(
            process_batch(&mut vec![job], &mut String::new(), &AtomicBool::new(false)),
            BatchOutcome::Retry
        );
        let saved = store.load(&group.group_id).expect("reload group");
        assert!(!saved.actors[0].enabled);
        assert!(cccc_runtime::status(&group.group_id, &actor.id).is_err());
    }

    #[test]
    fn work_claimed_for_a_gone_terminal_moves_to_the_saved_push_owner() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("saved push", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::Deepseek;
        actor.command = vec!["sh".into(), "-c".into(), "exit 1".into()];
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = serde_json::json!({"to":["peer1"],"text":"left","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("event data");
        cccc_core::ledger::append(&ledger_path, &event).expect("event");
        crate::ops::runtime_delivery::claim(&home, &group, &actor, &event.id, "pty", false)
            .expect("claim");
        let mut jobs = vec![DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event: event.clone(),
            claimed: true,
            transport: "pty",
        }];

        assert_eq!(
            process_batch_tracked(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false),
                &mut HashSet::new(),
                &mut WakeBudget::default(),
            ),
            BatchOutcome::Delivered
        );

        // The old claim is settled on its channel, and only then does the owner
        // that takes the work claim it on its own.
        assert!(jobs.is_empty());
        let settled = cccc_core::ledger::read_all(&ledger_path)
            .expect("ledger")
            .into_iter()
            .filter(|record| {
                record.kind == "runtime.delivery" && record.data["source_event_id"] == event.id
            })
            .map(|record| {
                (
                    record.data["state"].as_str().expect("state").to_owned(),
                    record.data["transport"]
                        .as_str()
                        .expect("transport")
                        .to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(settled[0], ("claimed".to_owned(), "pty".to_owned()));
        assert_eq!(settled[1], ("failed".to_owned(), "pty".to_owned()));
        assert_eq!(settled[2], ("claimed".to_owned(), "deepseek".to_owned()));
        super::super::actor_delivery::shutdown_actor(&group.group_id, &actor.id);
    }

    #[test]
    fn work_left_by_a_terminal_returns_to_a_saved_pull_on_its_own_channel() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("saved pull", "").expect("group");
        let mut actor = Actor::new("peer1");
        actor.runtime = ActorRuntime::WebModel;
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        let job = |text: &str| {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = serde_json::json!({"to":["peer1"],"text":text,"message_mode":"send"})
                .as_object()
                .cloned()
                .expect("event data");
            cccc_core::ledger::append(&ledger_path, &event).expect("event");
            crate::ops::runtime_delivery::claim(&home, &group, &actor, &event.id, "pty", false)
                .expect("claim");
            DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event,
                claimed: true,
                transport: "pty",
            }
        };
        let never_written = job("never written");
        let maybe_written = job("maybe written");
        let mut attempted = HashSet::from([maybe_written.event.id.clone()]);
        let mut jobs = vec![never_written.clone(), maybe_written.clone()];

        assert_eq!(
            process_batch_tracked(
                &mut jobs,
                &mut String::new(),
                &AtomicBool::new(false),
                &mut attempted,
                &mut WakeBudget::default(),
            ),
            BatchOutcome::Delivered
        );

        assert!(jobs.is_empty());
        let settled = |job: &DeliveryJob| {
            crate::ops::runtime_delivery::latest_state(
                &home,
                &group.group_id,
                &actor.id,
                &job.event.id,
            )
            .expect("delivery state")
            .expect("settled")
        };
        assert_eq!(settled(&never_written).1, "pty");
        assert_eq!(
            settled(&maybe_written),
            ("ambiguous".to_owned(), "pty".to_owned())
        );
        // Only what was never written is offered to the next pull.
        let pending = crate::ops::runtime_delivery::pending_sources(&home, &group, &actor, 10)
            .expect("pending work");
        assert_eq!(
            pending.iter().map(|event| &event.id).collect::<Vec<_>>(),
            [&never_written.event.id]
        );
    }

    #[test]
    fn scopeless_group_delivery_is_terminal_not_deferred() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        // No active scope and no actor default_scope_key: runtime can never
        // start, matching actor_new_session's missing_project_root.
        let mut group = store.create("scopeless delivery", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save group");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = serde_json::json!({"to":["peer1"],"text":"work"})
            .as_object()
            .cloned()
            .expect("event data");
        let job = DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor,
            event,
            claimed: true,
            transport: "managed_session",
        };

        match process_batch(&mut vec![job], &mut String::new(), &AtomicBool::new(false)) {
            BatchOutcome::Terminal(reason) => {
                assert!(
                    reason.contains("project root") || reason.contains("scope"),
                    "terminal reason should name the missing scope: {reason}"
                );
            }
            other => panic!("expected Terminal, got {other:?}"),
        }
    }
}

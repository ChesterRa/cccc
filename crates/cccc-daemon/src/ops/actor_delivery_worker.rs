use cccc_contracts::{Actor, ActorRuntime, GroupState};
use cccc_core::GroupStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ops::actor_delivery::{DeliveryJob, complete_job};
use crate::ops::actor_runtime;

const PREAMBLE_DELAY: Duration = Duration::from_millis(500);
const INPUT_MODE_TIMEOUT: Duration = Duration::from_secs(5);
const ANTIGRAVITY_STARTUP_SETTLE: Duration = Duration::from_millis(1_500);

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
pub fn process_batch(
    jobs: &mut Vec<DeliveryJob>,
    preamble_session: &mut String,
    cancelled: &AtomicBool,
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
    // Every attempt, including retries, re-checks Mail notices against their
    // Mail: one read, answered, or delivered since is withdrawn, never sent, and
    // leaves the batch for good.
    withdraw_stale_mail_notices(jobs, &job.home, &current_group, &current_actor);
    if jobs.is_empty() {
        return BatchOutcome::Delivered;
    }
    let jobs = jobs.as_slice();
    if current_actor.runtime == ActorRuntime::Deepseek {
        return process_deepseek_batch(jobs, &job.home, &current_group, &current_actor, cancelled);
    }
    if crate::ops::local_headless::uses_managed_delivery(&current_group.group_id, &current_actor) {
        return process_managed_batch(jobs, &job.home, &current_group, &current_actor, cancelled);
    }
    let status = match ensure_running(&job.home, &current_group, &current_actor) {
        Ok(Some(status)) => status,
        Ok(None) => return BatchOutcome::Retry,
        Err(reason) => return BatchOutcome::Terminal(reason),
    };
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
            if !submit_text(
                &current_group.group_id,
                &current_actor,
                &super::actor_delivery_preamble::render(&job.home, &current_group, &current_actor),
                cancelled,
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
    if submit_text(&current_group.group_id, &current_actor, &payload, cancelled) {
        preamble_session.clone_from(&status.started_at);
        finish_jobs(jobs);
        return BatchOutcome::Delivered;
    }
    BatchOutcome::Retry
}

fn process_deepseek_batch(
    jobs: &[DeliveryJob],
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    cancelled: &AtomicBool,
) -> BatchOutcome {
    if !crate::ops::deepseek_runtime::running(&group.group_id, &actor.id) {
        if crate::ops::deepseek_runtime::manual_restart_required(home, group, actor) {
            return BatchOutcome::Retry;
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
    for job in jobs {
        if cancelled.load(Ordering::Acquire)
            || !crate::ops::deepseek_runtime::deliver(home, group, actor, &job.event, cancelled)
        {
            return BatchOutcome::Retry;
        }
        complete_job(job);
    }
    BatchOutcome::Delivered
}

fn process_managed_batch(
    jobs: &[DeliveryJob],
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    cancelled: &AtomicBool,
) -> BatchOutcome {
    if !crate::ops::local_headless::running(&group.group_id, &actor.id) {
        match actor_runtime::apply(home, group, &actor.id, "actor.start") {
            Ok(None) if crate::ops::local_headless::running(&group.group_id, &actor.id) => {}
            Ok(_) => return BatchOutcome::Retry,
            Err(error) => {
                if let Some(reason) = terminal_reason(&error) {
                    return BatchOutcome::Terminal(reason);
                }
                if error.code == actor_runtime::CLAUDE_RESUME_FAILED {
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
    let outcome = crate::ops::local_headless::submit_batch(home, group, actor, &events, cancelled);
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

/// Record a managed submission's outcome. Returns false only for Deferred,
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
                    super::actor_delivery::delivery_transport(home, group, actor),
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

fn finish_jobs(jobs: &[DeliveryJob]) {
    for job in jobs {
        complete_job(job);
    }
}

fn ensure_running(
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
) -> Result<Option<cccc_runtime::SessionStatus>, String> {
    if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
        && status.running
    {
        return Ok(Some(status));
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

fn submit_text(group_id: &str, actor: &Actor, text: &str, cancelled: &AtomicBool) -> bool {
    super::actor_delivery::submit_terminal_text(group_id, actor, text, cancelled)
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
    use super::*;
    use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, Event};

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

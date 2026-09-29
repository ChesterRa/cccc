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

pub fn process_batch(
    jobs: &[DeliveryJob],
    preamble_session: &mut String,
    cancelled: &AtomicBool,
) -> BatchOutcome {
    let Some(job) = jobs.first() else {
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
    if current_actor.runtime == ActorRuntime::Deepseek {
        return process_deepseek_batch(jobs, &job.home, &current_group, &current_actor, cancelled);
    }
    if crate::ops::local_headless::supports(&current_actor) {
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
    if crate::ops::local_headless::submit_batch(home, group, actor, &events, cancelled) {
        finish_jobs(jobs);
        return BatchOutcome::Delivered;
    }
    BatchOutcome::Retry
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
            process_batch(&[job], &mut String::new(), &AtomicBool::new(false),),
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

        match process_batch(&[job], &mut String::new(), &AtomicBool::new(false)) {
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

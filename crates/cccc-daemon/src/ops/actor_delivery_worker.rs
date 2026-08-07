use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, GroupState};
use cccc_core::GroupStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ops::actor_delivery::{DeliveryCompletion, DeliveryJob, record_completion};
use crate::ops::actor_runtime;

const SUBMIT_DELAY: Duration = Duration::from_millis(1_500);
const REPEAT_SUBMIT_DELAY: Duration = Duration::from_millis(200);
const PREAMBLE_DELAY: Duration = Duration::from_millis(500);
const INPUT_MODE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn wait_for_delivery_slot(
    job: &DeliveryJob,
    last_delivery: &Option<std::time::Instant>,
    cancelled: &AtomicBool,
) -> bool {
    let Ok(group) =
        GroupStore::new(job.home.clone()).and_then(|store| store.load(&job.group.group_id))
    else {
        return false;
    };
    apply_throttle(&group, last_delivery, cancelled)
}

pub fn process_batch(
    jobs: &[DeliveryJob],
    preamble_session: &mut String,
    last_delivery: &mut Option<std::time::Instant>,
    cancelled: &AtomicBool,
) -> bool {
    let Some(job) = jobs.first() else {
        return false;
    };
    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    let Ok(current_group) =
        GroupStore::new(job.home.clone()).and_then(|store| store.load(&job.group.group_id))
    else {
        return false;
    };
    if matches!(
        current_group.state,
        GroupState::Paused | GroupState::Stopped
    ) {
        return false;
    }
    let Some(current_actor) = current_group
        .actors
        .iter()
        .find(|actor| actor.id == job.actor.id)
        .cloned()
    else {
        return false;
    };
    if crate::ops::local_headless::supports(&current_actor) {
        return process_headless_batch(
            jobs,
            &job.home,
            &current_group,
            &current_actor,
            last_delivery,
        );
    }
    let Some(status) = ensure_running(&job.home, &current_group, &current_actor) else {
        return false;
    };
    if *preamble_session != status.started_at {
        if current_actor.runtime != ActorRuntime::Custom
            && !wait_for_input_mode(&current_group.group_id, &current_actor.id, cancelled)
        {
            return false;
        }
        if !submit_text(
            &current_group.group_id,
            &current_actor,
            &super::actor_delivery_preamble::render(&job.home, &current_group, &current_actor),
            cancelled,
        ) {
            return false;
        }
        preamble_session.clone_from(&status.started_at);
        if !interruptible_sleep(PREAMBLE_DELAY, cancelled) {
            return false;
        }
    }

    let events = jobs.iter().map(|job| job.event.clone()).collect::<Vec<_>>();
    let Some(payload) = super::actor_delivery_render::render_batch(&events) else {
        return false;
    };
    if submit_text(&current_group.group_id, &current_actor, &payload, cancelled) {
        *last_delivery = Some(std::time::Instant::now());
        for job in jobs {
            record_completion(DeliveryCompletion {
                group_id: job.group.group_id.clone(),
                actor_id: job.actor.id.clone(),
                event_id: job.event.id.clone(),
            });
        }
        return true;
    }
    false
}

fn process_headless_batch(
    jobs: &[DeliveryJob],
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
    last_delivery: &mut Option<std::time::Instant>,
) -> bool {
    if !crate::ops::local_headless::running(&group.group_id, &actor.id) {
        match actor_runtime::apply(home, group, &actor.id, "actor.start") {
            Ok(None) if crate::ops::local_headless::running(&group.group_id, &actor.id) => {}
            Ok(_) => return false,
            Err(error) => {
                tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %error.message,
                    "failed to auto-wake headless actor for message delivery"
                );
                return false;
            }
        }
    }
    if !actor.enabled
        && let Err(error) = actor_runtime::persist_lifecycle(home, group, &actor.id, true, None)
    {
        crate::ops::local_headless::stop(&group.group_id, &actor.id);
        tracing::warn!(
            group_id = %group.group_id,
            actor_id = %actor.id,
            message = %error.message,
            "failed to persist auto-woken headless actor"
        );
        return false;
    }
    if jobs
        .iter()
        .all(|job| crate::ops::local_headless::submit(home, group, actor, &job.event))
    {
        *last_delivery = Some(std::time::Instant::now());
        for job in jobs {
            record_completion(DeliveryCompletion {
                group_id: job.group.group_id.clone(),
                actor_id: job.actor.id.clone(),
                event_id: job.event.id.clone(),
            });
        }
        return true;
    }
    false
}

fn ensure_running(
    home: &cccc_core::HomeLayout,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
) -> Option<cccc_runtime::SessionStatus> {
    if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
        && status.running
    {
        return Some(status);
    }
    let status = match actor_runtime::apply(home, group, &actor.id, "actor.start") {
        Ok(Some(status)) if status.running => status,
        Ok(_) => return None,
        Err(error) => {
            if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
                && status.running
            {
                status
            } else {
                tracing::warn!(
                    group_id = %group.group_id,
                    actor_id = %actor.id,
                    message = %error.message,
                    "failed to auto-wake actor for message delivery"
                );
                return None;
            }
        }
    };
    if !actor.enabled
        && let Err(error) =
            actor_runtime::persist_lifecycle(home, group, &actor.id, true, Some(&status))
    {
        let _ = actor_runtime::stop_if_started_at(group, &status);
        tracing::warn!(
            group_id = %group.group_id,
            actor_id = %actor.id,
            message = %error.message,
            "failed to persist auto-woken actor"
        );
        return None;
    }
    Some(status)
}

fn apply_throttle(
    group: &cccc_core::GroupDoc,
    last_delivery: &Option<std::time::Instant>,
    cancelled: &AtomicBool,
) -> bool {
    let seconds = super::actor_delivery::delivery_setting(group, "min_interval_seconds")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let Some(remaining) =
        last_delivery.and_then(|last| Duration::from_secs(seconds).checked_sub(last.elapsed()))
    else {
        return true;
    };
    interruptible_sleep(remaining, cancelled)
}

fn submit_text(group_id: &str, actor: &Actor, text: &str, cancelled: &AtomicBool) -> bool {
    let raw = text.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return false;
    }
    let bracketed = raw.contains(['\r', '\n'])
        && cccc_runtime::bracketed_paste_enabled(group_id, &actor.id).unwrap_or(false);
    let payload = if bracketed {
        format!("\u{1b}[200~{raw}\u{1b}[201~")
    } else if raw.contains(['\r', '\n']) {
        raw.lines().collect::<Vec<_>>().join(" ")
    } else {
        raw.to_owned()
    };
    let submits = submit_sequence(actor);
    let delay = if submits.is_empty() {
        Duration::ZERO
    } else {
        SUBMIT_DELAY
    };
    cccc_runtime::submit_sequence_interruptible(
        group_id,
        &actor.id,
        payload.as_bytes(),
        submits,
        delay,
        REPEAT_SUBMIT_DELAY,
        cancelled,
    )
    .unwrap_or(false)
}

fn submit_sequence(actor: &Actor) -> &'static [&'static [u8]] {
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
    use super::submit_sequence;
    use cccc_contracts::{Actor, ActorRuntime, ActorSubmit};

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
}

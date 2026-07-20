use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, GroupState};
use cccc_core::{GroupStore, system_prompt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ops::actor_delivery::{DeliveryCompletion, DeliveryJob, record_completion};

const SUBMIT_DELAY: Duration = Duration::from_millis(1_500);
const PREAMBLE_DELAY: Duration = Duration::from_millis(500);
const INPUT_MODE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn process(
    job: &DeliveryJob,
    preamble_session: &mut String,
    last_delivery: &mut Option<std::time::Instant>,
    cancelled: &AtomicBool,
) -> bool {
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
    let Ok(status) = cccc_runtime::status(&job.group.group_id, &job.actor.id) else {
        return false;
    };
    if !status.running {
        return false;
    }
    if !apply_throttle(&current_group, last_delivery, cancelled) {
        return false;
    }

    if job.actor.runtime != ActorRuntime::Custom && *preamble_session != status.started_at {
        if !wait_for_input_mode(&job.group.group_id, &job.actor.id, cancelled) {
            return false;
        }
        if !submit_text(
            &job.group.group_id,
            &job.actor,
            &system_prompt::render(&current_group, &job.actor),
            cancelled,
        ) {
            return false;
        }
        preamble_session.clone_from(&status.started_at);
        if !interruptible_sleep(PREAMBLE_DELAY, cancelled) {
            return false;
        }
    }

    let Some(payload) = super::actor_delivery_render::render(&job.event) else {
        return false;
    };
    if submit_text(&job.group.group_id, &job.actor, &payload, cancelled) {
        *last_delivery = Some(std::time::Instant::now());
        record_completion(DeliveryCompletion {
            group_id: job.group.group_id.clone(),
            actor_id: job.actor.id.clone(),
            event_id: job.event.id.clone(),
        });
        return true;
    }
    false
}

fn apply_throttle(
    group: &cccc_core::GroupDoc,
    last_delivery: &Option<std::time::Instant>,
    cancelled: &AtomicBool,
) -> bool {
    let seconds = group
        .extra
        .get("settings")
        .and_then(|value| value.get("min_interval_seconds"))
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
    let submit = match actor.submit {
        ActorSubmit::Enter => b"\r".as_slice(),
        ActorSubmit::Newline => b"\n".as_slice(),
        ActorSubmit::None => b"".as_slice(),
    };
    let delay = if submit.is_empty() {
        Duration::ZERO
    } else {
        SUBMIT_DELAY
    };
    cccc_runtime::submit_interruptible(
        group_id,
        &actor.id,
        payload.as_bytes(),
        submit,
        delay,
        cancelled,
    )
    .unwrap_or(false)
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

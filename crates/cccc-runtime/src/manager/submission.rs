//! Keep a submission on the exact PTY instance whose input policy was checked.
use super::write_locked;
use crate::cancellation::{lock_interruptibly, wait_interruptibly};
use crate::registry::lookup;
use crate::{RuntimeError, SessionStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub fn submit_sequence_interruptible(
    group_id: &str,
    actor_id: &str,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    submit_sequence(
        (group_id, actor_id),
        None,
        payload,
        submits,
        initial_delay,
        repeat_delay,
        cancelled,
    )
}

/// Return false without writing if the Actor now has a different terminal.
pub fn submit_sequence_for_session_interruptible(
    expected: &SessionStatus,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    submit_sequence(
        (&expected.group_id, &expected.actor_id),
        Some(expected),
        payload,
        submits,
        initial_delay,
        repeat_delay,
        cancelled,
    )
}

fn submit_sequence(
    target: (&str, &str),
    expected: Option<&SessionStatus>,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    let (group_id, actor_id) = target;
    if cancelled.load(Ordering::Acquire) {
        return Ok(false);
    }
    let session = lookup(group_id, actor_id)?;
    let gate = session
        .lock()
        .map_err(|_| RuntimeError::Poisoned)?
        .input_gate();
    let Some(_guard) = lock_interruptibly(&gate, &|| cancelled.load(Ordering::Acquire))? else {
        return Ok(false);
    };
    if cancelled.load(Ordering::Acquire) {
        return Ok(false);
    }
    if !write_locked(&session, payload, Some(cancelled), expected)? {
        return Ok(false);
    }
    for (index, submit) in submits
        .iter()
        .filter(|submit| !submit.is_empty())
        .enumerate()
    {
        let delay = if index == 0 {
            initial_delay
        } else {
            repeat_delay
        };
        if !wait_interruptibly(delay, cancelled) {
            return Ok(false);
        }
        if !write_locked(&session, submit, Some(cancelled), expected)? {
            return Ok(false);
        }
    }
    Ok(true)
}

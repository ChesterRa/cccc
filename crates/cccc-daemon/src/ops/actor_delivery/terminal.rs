//! Format terminal input once for managed viewers and generation-bound ordinary workers.
use super::{TERMINAL_REPEAT_SUBMIT_DELAY, TERMINAL_SUBMIT_DELAY, terminal_submit_sequence};
use crate::ops::local_headless;
use cccc_contracts::Actor;
use cccc_runtime::SessionStatus;
use std::sync::atomic::AtomicBool;

/// Managed viewers retain their native terminal submission path.
pub(in crate::ops) fn submit_terminal_text(
    group_id: &str,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
) -> bool {
    submit_terminal_text_inner(group_id, actor, text, cancelled, None)
}

/// Ordinary delivery may write only to the PTY instance already classified by its worker.
pub(in crate::ops) fn submit_terminal_text_for_session(
    expected: &SessionStatus,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
) -> bool {
    if actor.id != expected.actor_id {
        return false;
    }
    submit_terminal_text_inner(&expected.group_id, actor, text, cancelled, Some(expected))
}

fn submit_terminal_text_inner(
    group_id: &str,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
    expected: Option<&SessionStatus>,
) -> bool {
    let raw = text.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return false;
    }
    if local_headless::running(group_id, &actor.id)
        && !cccc_runtime::wait_for_input_ready(
            group_id,
            &actor.id,
            std::time::Duration::from_secs(15),
            cancelled,
        )
        .unwrap_or(false)
    {
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
    let submits = terminal_submit_sequence(actor);
    let delay = if submits.is_empty() {
        std::time::Duration::ZERO
    } else {
        TERMINAL_SUBMIT_DELAY
    };
    match expected {
        Some(expected) => cccc_runtime::submit_sequence_for_session_interruptible(
            expected,
            payload.as_bytes(),
            submits,
            delay,
            TERMINAL_REPEAT_SUBMIT_DELAY,
            cancelled,
        ),
        None => cccc_runtime::submit_sequence_interruptible(
            group_id,
            &actor.id,
            payload.as_bytes(),
            submits,
            delay,
            TERMINAL_REPEAT_SUBMIT_DELAY,
            cancelled,
        ),
    }
    .unwrap_or(false)
}

//! Format terminal input once for managed viewers and generation-bound ordinary workers.
use super::{TERMINAL_REPEAT_SUBMIT_DELAY, TERMINAL_SUBMIT_DELAY, terminal_submit_sequence};
use crate::ops::local_headless;
use crate::session_owner::runtime as cccc_runtime;
use cccc_contracts::Actor;
use cccc_runtime::{PtyHandoff, PtyHandoffReason, SessionStatus};
use std::sync::atomic::AtomicBool;

/// Ordinary delivery and managed viewers may write only to the observed PTY
/// instance, and learn whether a failed attempt left anything in that terminal.
pub(crate) fn submit_terminal_text_for_session(
    expected: &SessionStatus,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
) -> PtyHandoff {
    submit_viewer_text(expected, false, actor, text, cancelled)
}

pub(crate) fn submit_viewer_text(
    expected: &SessionStatus,
    needs_kilo_composer: bool,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
) -> PtyHandoff {
    if actor.id != expected.actor_id {
        return PtyHandoff::NotWritten(PtyHandoffReason::SessionChanged);
    }
    let Some(input) = terminal_input(&expected.group_id, actor, text, cancelled) else {
        return PtyHandoff::NotWritten(PtyHandoffReason::Cancelled);
    };
    let submit = if needs_kilo_composer {
        cccc_runtime::submit_sequence_for_new_kilo_viewer
    } else {
        cccc_runtime::submit_sequence_for_session_with_evidence_interruptible
    };
    submit(
        expected,
        input.payload.as_bytes(),
        input.submits,
        input.delay,
        TERMINAL_REPEAT_SUBMIT_DELAY,
        cancelled,
    )
}

struct TerminalInput {
    payload: String,
    submits: &'static [&'static [u8]],
    delay: std::time::Duration,
}

/// `None` when there is nothing to write or the terminal never became ready.
fn terminal_input(
    group_id: &str,
    actor: &Actor,
    text: &str,
    cancelled: &AtomicBool,
) -> Option<TerminalInput> {
    let raw = text.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return None;
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
        return None;
    }
    let multiline = raw.contains(['\r', '\n']);
    let bracketed =
        multiline && cccc_runtime::bracketed_paste_enabled(group_id, &actor.id).unwrap_or(false);
    if multiline && !bracketed && actor.runtime == cccc_contracts::ActorRuntime::Kilo {
        return None;
    }
    let payload = if bracketed {
        format!("\u{1b}[200~{raw}\u{1b}[201~")
    } else if multiline {
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
    Some(TerminalInput {
        payload,
        submits,
        delay,
    })
}

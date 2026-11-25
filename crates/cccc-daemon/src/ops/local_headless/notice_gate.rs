//! Backpressure for early Mail notices. Mail is meant not to interrupt work.
//! An early notice from a Mail write or the unread tick carries `deliver_by`,
//! the ordinary notice deadline. Before that deadline, managed and PTY handoff
//! recheck idle evidence for batches consisting entirely of early notices.
//! Without that evidence the delivery is withdrawn. The unread tick can issue
//! a fresh notice once the Actor is idle if the Mail is still unread.
//! If recording the withdrawal fails, only the record is retried. A daemon
//! that dies between claiming a notice and recording its outcome leaves it
//! ambiguous, and that notice is not re-issued.
//!
//! Managed idle comes from the Runtime's turn and approval state. A PTY can
//! report idle through an existing, matching Antigravity hook binding.
//! Admission and the terminal write are not atomic, and a delivery
//! written moments before the Runtime starts on it can share its turn with a
//! notice; that window is accepted for simplicity.

use cccc_contracts::Event;
use cccc_core::inbox::targeted_prompt_kind;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{BatchSubmission, Session};

/// An early Mail notice whose `deliver_by` has not passed yet.
fn held_notice(event: &Event, now: DateTime<Utc>) -> bool {
    targeted_prompt_kind(event) == Some("mail_notice")
        && event
            .data
            .get("context")
            .and_then(|context| context.get("deliver_by"))
            .and_then(Value::as_str)
            .and_then(|deadline| DateTime::parse_from_rfc3339(deadline).ok())
            .is_some_and(|deadline| now < deadline)
}

/// Batches with any other event carry work the sender wants delivered now.
pub(crate) fn held_batch(events: &[Event], now: DateTime<Utc>) -> bool {
    !events.is_empty() && events.iter().all(|event| held_notice(event, now))
}

impl Session {
    pub(super) fn ready_for_mail_notice(&self) -> bool {
        self.active_turn.lock().is_ok_and(|turn| turn.is_none())
            && self.status.lock().is_ok_and(|state| state.status == "idle")
    }

    /// Submit `events` unless they are held notices the Actor is not ready
    /// for, which are withheld instead.
    pub(super) fn admit_batch(
        &self,
        events: &[Event],
        submit: impl FnOnce() -> BatchSubmission,
    ) -> BatchSubmission {
        if held_batch(events, Utc::now()) && !self.ready_for_mail_notice() {
            return BatchSubmission::Withheld;
        }
        submit()
    }
}

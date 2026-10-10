//! Backpressure for early Mail notices. Mail is meant not to interrupt work.
//! A notice the unread tick sent early, because the Actor looked idle, carries
//! `deliver_by`: the time the busy delay would have sent it anyway. Until then a
//! batch made only of such notices is admitted to a managed Actor only while it
//! is idle; otherwise it is withheld: the delivery is withdrawn without sending
//! anything, and the unread tick issues a fresh notice once the Actor is idle,
//! provided the Mail is still unread, so Mail read in the meantime is never
//! announced. A held notice does not wait in the delivery queue; if recording
//! the withdrawal fails, only the record is retried. As for any delivery, a
//! daemon that dies between claiming a notice and recording its outcome leaves
//! it ambiguous, and that notice is not re-issued.
//!
//! Idle is what the Runtime reports: no turn running and not waiting for
//! approval. An early notice is held while the Runtime reports work when it is
//! admitted. Admission and the terminal write are not atomic, and a delivery
//! written moments before the Runtime starts on it can share its turn with a
//! notice; that window is accepted for simplicity.

use cccc_contracts::Event;
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{BatchSubmission, Session};

/// An early Mail notice whose `deliver_by` has not passed yet.
fn held_notice(event: &Event, now: DateTime<Utc>) -> bool {
    event.kind == "system.notify"
        && event.data.get("kind").and_then(Value::as_str) == Some("mail_notice")
        && event
            .data
            .get("context")
            .and_then(|context| context.get("deliver_by"))
            .and_then(Value::as_str)
            .and_then(|deadline| DateTime::parse_from_rfc3339(deadline).ok())
            .is_some_and(|deadline| now < deadline)
}

/// Batches with any other event carry work the sender wants delivered now.
pub(super) fn held_batch(events: &[Event], now: DateTime<Utc>) -> bool {
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

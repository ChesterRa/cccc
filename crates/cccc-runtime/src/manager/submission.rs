//! Keep a submission on the exact PTY instance whose input policy was checked.
use crate::cancellation::{lock_interruptibly, wait_interruptibly};
use crate::registry::{SharedSession, lookup};
use crate::{RuntimeError, SessionStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Whether a delivery may be retried without risking a duplicate.
#[derive(Debug)]
pub enum PtyHandoff {
    /// The payload and every configured submission key were written.
    Accepted,
    /// No byte was written to the PTY.
    NotWritten(PtyHandoffReason),
    /// At least one byte was written but the submission did not finish.
    Uncertain(PtyHandoffReason),
}

#[derive(Debug, thiserror::Error)]
pub enum PtyHandoffReason {
    #[error("terminal submission was cancelled")]
    Cancelled,
    #[error("terminal session changed before submission finished")]
    SessionChanged,
    #[error("{0}")]
    Runtime(RuntimeError),
}

impl PtyHandoff {
    fn into_legacy(self) -> Result<bool, RuntimeError> {
        match self {
            Self::Accepted => Ok(true),
            Self::NotWritten(PtyHandoffReason::Runtime(error))
            | Self::Uncertain(PtyHandoffReason::Runtime(error)) => Err(error),
            Self::NotWritten(_) | Self::Uncertain(_) => Ok(false),
        }
    }
}

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
    .into_legacy()
}

/// Return false on cancellation or a changed terminal; this does not report
/// whether an earlier part of the submission was written.
pub fn submit_sequence_for_session_interruptible(
    expected: &SessionStatus,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> Result<bool, RuntimeError> {
    submit_sequence_for_session_with_evidence_interruptible(
        expected,
        payload,
        submits,
        initial_delay,
        repeat_delay,
        cancelled,
    )
    .into_legacy()
}

/// Submit to one observed PTY and retain evidence when cancellation or I/O interrupts it.
pub fn submit_sequence_for_session_with_evidence_interruptible(
    expected: &SessionStatus,
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
) -> PtyHandoff {
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
) -> PtyHandoff {
    if cancelled.load(Ordering::Acquire) {
        return PtyHandoff::NotWritten(PtyHandoffReason::Cancelled);
    }
    let session = match lookup(target.0, target.1) {
        Ok(session) => session,
        Err(error) => return PtyHandoff::NotWritten(PtyHandoffReason::Runtime(error)),
    };
    let gate = match session.lock() {
        Ok(session) => session.input_gate(),
        Err(_) => return PtyHandoff::NotWritten(PtyHandoffReason::Runtime(RuntimeError::Poisoned)),
    };
    let _guard = match lock_interruptibly(&gate, &|| cancelled.load(Ordering::Acquire)) {
        Ok(Some(guard)) => guard,
        Ok(None) => return PtyHandoff::NotWritten(PtyHandoffReason::Cancelled),
        Err(error) => return PtyHandoff::NotWritten(PtyHandoffReason::Runtime(error)),
    };
    if cancelled.load(Ordering::Acquire) {
        return PtyHandoff::NotWritten(PtyHandoffReason::Cancelled);
    }
    submit_writes(
        payload,
        submits,
        initial_delay,
        repeat_delay,
        cancelled,
        |data| write_tracked(&session, data, cancelled, expected),
    )
}

struct Attempt {
    written: usize,
    result: Result<(), PtyHandoffReason>,
}

fn write_tracked(
    session: &SharedSession,
    data: &[u8],
    cancelled: &AtomicBool,
    expected: Option<&SessionStatus>,
) -> Attempt {
    let get_writer = || {
        let mut current = session.lock().map_err(|_| RuntimeError::Poisoned)?;
        if let Some(expected) = expected {
            let actual = current.status();
            if actual.pid != expected.pid || actual.started_at != expected.started_at {
                return Ok(None);
            }
        }
        current.input_writer().map(Some)
    };
    let writer = match get_writer() {
        Ok(Some(writer)) => writer,
        Ok(None) => {
            return Attempt {
                written: 0,
                result: Err(PtyHandoffReason::SessionChanged),
            };
        }
        Err(error) => {
            return Attempt {
                written: 0,
                result: Err(PtyHandoffReason::Runtime(error)),
            };
        }
    };
    // The input gate preserves ordering while this pins the exact PTY writer.
    match crate::pty_input::write_input_interruptibly_tracked(&writer, data, &|| {
        cancelled.load(Ordering::Acquire)
    }) {
        Ok(progress) => attempt_from_progress(progress),
        Err(error) => Attempt {
            written: 0,
            result: Err(PtyHandoffReason::Runtime(error)),
        },
    }
}

fn attempt_from_progress(progress: crate::pty_input::WriteProgress) -> Attempt {
    Attempt {
        written: progress.written,
        result: match progress.result {
            Ok(true) => Ok(()),
            Ok(false) => Err(PtyHandoffReason::Cancelled),
            Err(error) => Err(PtyHandoffReason::Runtime(error.into())),
        },
    }
}

fn submit_writes(
    payload: &[u8],
    submits: &[&[u8]],
    initial_delay: Duration,
    repeat_delay: Duration,
    cancelled: &AtomicBool,
    mut write: impl FnMut(&[u8]) -> Attempt,
) -> PtyHandoff {
    let mut written = 0;
    let attempt = write(payload);
    written += attempt.written;
    if let Err(reason) = attempt.result {
        return if written == 0 {
            PtyHandoff::NotWritten(reason)
        } else {
            PtyHandoff::Uncertain(reason)
        };
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
            return if written == 0 {
                PtyHandoff::NotWritten(PtyHandoffReason::Cancelled)
            } else {
                PtyHandoff::Uncertain(PtyHandoffReason::Cancelled)
            };
        }
        let attempt = write(submit);
        written += attempt.written;
        if let Err(reason) = attempt.result {
            return if written == 0 {
                PtyHandoff::NotWritten(reason)
            } else {
                PtyHandoff::Uncertain(reason)
            };
        }
    }
    PtyHandoff::Accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty_input::PtyInput;
    use std::collections::VecDeque;
    use std::io::{self, Write};

    enum Step {
        Write(usize),
        CancelAfter(usize),
        Error,
    }

    struct FakeInput<'a> {
        steps: VecDeque<Step>,
        bytes: Vec<u8>,
        cancelled: &'a AtomicBool,
        fail_flush: bool,
    }

    impl Write for FakeInput<'_> {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            let (count, cancel) = match self.steps.pop_front() {
                Some(Step::Write(count)) => (count, false),
                Some(Step::CancelAfter(count)) => (count, true),
                Some(Step::Error) => return Err(io::ErrorKind::BrokenPipe.into()),
                None => (data.len(), false),
            };
            let count = count.min(data.len());
            self.bytes.extend_from_slice(&data[..count]);
            if cancel {
                self.cancelled.store(true, Ordering::Release);
            }
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(())
            }
        }
    }

    impl PtyInput for FakeInput<'_> {}

    fn run(steps: Vec<Step>, fail_flush: bool, submits: &[&[u8]]) -> (PtyHandoff, Vec<u8>) {
        let cancelled = AtomicBool::new(false);
        let mut input = FakeInput {
            steps: steps.into(),
            bytes: Vec::new(),
            cancelled: &cancelled,
            fail_flush,
        };
        let result =
            submit_writes(
                b"payload",
                submits,
                Duration::from_millis(1),
                Duration::from_millis(1),
                &cancelled,
                |data| {
                    attempt_from_progress(input.write_all_interruptible_tracked(data, &|| {
                        cancelled.load(Ordering::Acquire)
                    }))
                },
            );
        (result, input.bytes)
    }

    #[test]
    fn classifies_zero_and_partial_payload_errors() {
        let (zero, bytes) = run(vec![Step::Error], false, &[b"\r"]);
        assert!(matches!(
            zero,
            PtyHandoff::NotWritten(PtyHandoffReason::Runtime(_))
        ));
        assert!(bytes.is_empty());

        let (partial, bytes) = run(vec![Step::Write(2), Step::Error], false, &[b"\r"]);
        assert!(matches!(
            partial,
            PtyHandoff::Uncertain(PtyHandoffReason::Runtime(_))
        ));
        assert_eq!(bytes, b"pa");
    }

    #[test]
    fn cancellation_after_payload_or_partial_write_is_uncertain() {
        let (partial, bytes) = run(vec![Step::CancelAfter(2)], false, &[b"\r"]);
        assert!(matches!(
            partial,
            PtyHandoff::Uncertain(PtyHandoffReason::Cancelled)
        ));
        assert_eq!(bytes, b"pa");

        let (before_enter, bytes) = run(vec![Step::CancelAfter(7)], false, &[b"\r"]);
        assert!(matches!(
            before_enter,
            PtyHandoff::Uncertain(PtyHandoffReason::Cancelled)
        ));
        assert_eq!(bytes, b"payload");
    }

    #[test]
    fn submit_key_and_flush_failures_are_uncertain_after_payload() {
        let (key_error, bytes) = run(vec![Step::Write(7), Step::Error], false, &[b"\r"]);
        assert!(matches!(
            key_error,
            PtyHandoff::Uncertain(PtyHandoffReason::Runtime(_))
        ));
        assert_eq!(bytes, b"payload");

        let (flush_error, bytes) = run(vec![Step::Write(7)], true, &[b"\r"]);
        assert!(matches!(
            flush_error,
            PtyHandoff::Uncertain(PtyHandoffReason::Runtime(_))
        ));
        assert_eq!(bytes, b"payload");
    }

    #[test]
    fn accepts_only_after_all_submission_keys_are_written() {
        let (accepted, bytes) = run(vec![], false, &[b"\r", b"\r"]);
        assert!(matches!(accepted, PtyHandoff::Accepted));
        assert_eq!(bytes, b"payload\r\r");
    }

    #[test]
    fn legacy_interface_preserves_failure_results() {
        assert!(matches!(
            PtyHandoff::NotWritten(PtyHandoffReason::Cancelled).into_legacy(),
            Ok(false)
        ));
        assert!(matches!(
            PtyHandoff::Uncertain(PtyHandoffReason::Runtime(
                io::Error::from(io::ErrorKind::BrokenPipe).into()
            ))
            .into_legacy(),
            Err(RuntimeError::Io(_))
        ));
    }

    #[test]
    fn cancellation_before_session_lookup_does_not_write() {
        let cancelled = AtomicBool::new(true);
        let result = submit_sequence(
            ("absent-group", "absent-actor"),
            None,
            b"payload",
            &[b"\r"],
            Duration::ZERO,
            Duration::ZERO,
            &cancelled,
        );
        assert!(matches!(
            result,
            PtyHandoff::NotWritten(PtyHandoffReason::Cancelled)
        ));
    }
}

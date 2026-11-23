//! Per-connection retry deadlines; a failed peer never delays another connection.
//! Inspired by the capped, jittered backoff in [iroh's relay actor].
//!
//! [iroh's relay actor]: https://github.com/n0-computer/iroh/blob/a5e39c44d3d6dfaaff30b7292126e06ea0ab7a21/iroh/src/socket/transports/relay/actor.rs#L330-L399
use std::time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub(super) struct RetryState {
    failures: u32,
    next: Option<Instant>,
}

impl RetryState {
    pub(super) fn ready(&self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    pub(super) fn connected(&mut self) {
        self.failures = 0;
        self.next = None;
    }

    pub(super) fn completed(&mut self, now: Instant, success: bool, awaiting_expired: bool) {
        let delay = if success {
            self.connected();
            Duration::from_secs(5)
        } else {
            let ceiling = (5_000_u64 << self.failures.min(4)).min(60_000);
            self.failures = self.failures.saturating_add(1);
            // Equal jitter keeps a non-zero floor while spreading retries, including at the cap.
            let half = ceiling / 2;
            let jitter = (uuid::Uuid::new_v4().as_u128() % u128::from(half + 1)) as u64;
            Duration::from_millis(half + jitter)
        };
        // Only the receiver can confirm whether approval preceded the invitation deadline.
        let delay = if awaiting_expired {
            delay.max(Duration::from_secs(60))
        } else {
            delay
        };
        self.next = Some(now + delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_have_bounded_backoff_and_reset_on_connection() {
        let now = Instant::now();
        let mut retry = RetryState::default();
        for ceiling_ms in [5_000, 10_000, 20_000, 40_000, 60_000, 60_000] {
            retry.completed(now, false, false);
            assert!(!retry.ready(now + Duration::from_millis(ceiling_ms / 2 - 1)));
            assert!(retry.ready(now + Duration::from_millis(ceiling_ms)));
        }
        retry.connected();
        retry.completed(now, false, false);
        assert!(!retry.ready(now + Duration::from_millis(2_499)));
        assert!(retry.ready(now + Duration::from_secs(5)));
    }

    #[test]
    fn pending_approval_keeps_its_cadence_and_expired_requests_reconcile_slowly() {
        let now = Instant::now();
        let mut retry = RetryState::default();
        for _ in 0..6 {
            retry.completed(now, false, false);
        }
        retry.completed(now, true, false);
        assert!(!retry.ready(now + Duration::from_secs(4)));
        assert!(retry.ready(now + Duration::from_secs(5)));
        for success in [true, false] {
            retry.completed(now, success, true);
            assert!(!retry.ready(now + Duration::from_secs(59)));
            assert!(retry.ready(now + Duration::from_secs(60)));
        }
    }
}

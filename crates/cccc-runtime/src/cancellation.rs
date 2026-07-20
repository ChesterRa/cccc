use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) fn wait_interruptibly(delay: Duration, cancelled: &AtomicBool) -> bool {
    let deadline = std::time::Instant::now() + delay;
    while !cancelled.load(Ordering::Acquire) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
    false
}

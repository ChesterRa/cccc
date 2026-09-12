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

/// Waiting for another input transaction must remain cancellable too.
pub(crate) fn lock_interruptibly<'a, T>(
    mutex: &'a std::sync::Mutex<T>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<std::sync::MutexGuard<'a, T>>, crate::RuntimeError> {
    loop {
        if cancelled() {
            return Ok(None);
        }
        match mutex.try_lock() {
            Ok(guard) => return Ok(Some(guard)),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(crate::RuntimeError::Poisoned),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

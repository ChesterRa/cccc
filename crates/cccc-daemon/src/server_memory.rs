//! Return reclaimable glibc pages without changing the process's arena limit.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const PERIOD: Duration = Duration::from_secs(5 * 60);
const SLOW_TRIM: Duration = Duration::from_millis(200);

pub(super) struct MemoryMaintenance {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MemoryMaintenance {
    pub(super) fn start() -> Self {
        Self::start_with(PERIOD, trim)
    }

    fn start_with(period: Duration, trim: impl Fn(bool) + Send + Sync + 'static) -> Self {
        let (stop, mut stopped) = watch::channel(false);
        let trim = Arc::new(trim);
        let task = tokio::spawn(async move {
            let mut interval = maintenance_interval(period);
            let mut first = true;
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => break,
                    _ = interval.tick() => {}
                }
                if *stopped.borrow() || stopped.has_changed().is_err() {
                    break;
                }
                let trim = Arc::clone(&trim);
                // Await the blocking call: intervals cannot overlap, and normal
                // shutdown waits for an already-started trim to finish.
                if let Err(error) = tokio::task::spawn_blocking(move || trim(first)).await {
                    tracing::warn!(diagnostic_action = "memory_trim", diagnostic_code = "runtime_error", %error, "allocator maintenance stopped");
                    break;
                }
                first = false;
            }
        });
        Self { stop, task }
    }

    pub(super) async fn finish(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn maintenance_interval(period: Duration) -> tokio::time::Interval {
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

fn resident_kib() -> Option<u64> {
    // VmRSS is the kernel's inexpensive estimate, not a page-table walk.
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            (fields.next()? == "VmRSS:").then_some(())?;
            let value = fields.next()?.parse().ok()?;
            (fields.next()? == "kB").then_some(value)
        })
}

fn trim(first: bool) {
    let before = resident_kib();
    let started = Instant::now();
    let released = cccc_linux_memory::trim_unused_pages();
    let elapsed = started.elapsed();
    let after = resident_kib();
    let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    if first || elapsed > SLOW_TRIM {
        tracing::info!(target: "cccc_daemon::memory_trim", diagnostic_action = "memory_trim", duration_ms, rss_before_kib = before, rss_after_kib = after, trim_released = released, "allocator maintenance completed");
    } else {
        tracing::debug!(target: "cccc_daemon::memory_trim", diagnostic_action = "memory_trim", duration_ms, rss_before_kib = before, rss_after_kib = after, trim_released = released, "allocator maintenance completed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};

    #[tokio::test]
    async fn first_tick_waits_and_an_unused_service_stops_without_trimming() {
        let mut interval = maintenance_interval(PERIOD);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), interval.tick())
                .await
                .is_err(),
            "maintenance must not run immediately at daemon startup"
        );
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let service = MemoryMaintenance::start_with(PERIOD, move |_| {
            let _ = sent.send(());
        });
        tokio::time::timeout(Duration::from_secs(5), service.finish())
            .await
            .expect("unused maintenance task stops");
        assert!(received.try_recv().is_err());
    }

    #[tokio::test]
    async fn recurring_calls_mark_only_the_first_and_owner_drop_stops_the_task() {
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let service = MemoryMaintenance::start_with(Duration::from_millis(10), move |first| {
            let _ = sent.send(first);
        });
        for expected in [true, false] {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), received.recv())
                    .await
                    .expect("recurring maintenance call"),
                Some(expected)
            );
        }
        let MemoryMaintenance { stop, task } = service;
        drop(stop);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("owner cancellation stops maintenance")
            .expect("maintenance task");
    }

    struct ReleaseOnDrop(Arc<(Mutex<bool>, Condvar)>);

    impl ReleaseOnDrop {
        fn release(&self) {
            *self.0.0.lock().expect("release gate") = true;
            self.0.1.notify_all();
        }
    }

    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[tokio::test]
    async fn trims_do_not_overlap_and_shutdown_joins_the_running_call() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = ReleaseOnDrop(Arc::clone(&gate));
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let service = MemoryMaintenance::start_with(Duration::from_millis(10), move |first| {
            let _ = sent.send(first);
            let (lock, condition) = &*gate;
            let guard = lock.lock().expect("callback gate");
            drop(
                condition
                    .wait_while(guard, |released| !*released)
                    .expect("wait for release"),
            );
        });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), received.recv())
                .await
                .expect("first maintenance call"),
            Some(true)
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
        let overlapped = received.try_recv().is_ok();
        let mut finish = Box::pin(service.finish());
        let ended_while_blocked = tokio::time::timeout(Duration::from_millis(20), &mut finish)
            .await
            .is_ok();
        release.release();
        if !ended_while_blocked {
            tokio::time::timeout(Duration::from_secs(5), finish)
                .await
                .expect("maintenance joins after release");
        }
        assert!(!overlapped, "a slow trim must not queue another call");
        assert!(!ended_while_blocked, "shutdown must await the current call");
        assert!(received.try_recv().is_err(), "no trim after shutdown");
    }
}

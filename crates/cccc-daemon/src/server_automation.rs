use cccc_core::HomeLayout;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

use crate::dispatch_concurrency::DispatchLocks;

const GROUP_CONCURRENCY: usize = 4;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

pub struct AutomationScheduler {
    task: Option<JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
    last_unread_tick: Instant,
}

impl AutomationScheduler {
    const UNREAD_INTERVAL: Duration = Duration::from_secs(60);

    pub fn new() -> Self {
        Self {
            task: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            last_unread_tick: Instant::now(),
        }
    }

    pub fn trigger(&mut self, home: HomeLayout, locks: DispatchLocks) {
        if self.task.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        self.task.take();
        let include_unread = self.take_unread_due();
        self.task = Some(tokio::spawn(run(
            home,
            locks,
            include_unread,
            Arc::clone(&self.cancelled),
        )));
    }

    pub async fn finish(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
            tracing::warn!("automation shutdown timed out; cancelled the active maintenance task");
        }
    }

    fn take_unread_due(&mut self) -> bool {
        if self.last_unread_tick.elapsed() < Self::UNREAD_INTERVAL {
            return false;
        }
        self.last_unread_tick = Instant::now();
        true
    }
}

async fn run(
    home: HomeLayout,
    locks: DispatchLocks,
    include_unread: bool,
    cancelled: Arc<AtomicBool>,
) {
    if cancelled.load(Ordering::Acquire) {
        return;
    }
    let exited = prepare_exited().await;
    if cancelled.load(Ordering::Acquire) {
        return;
    }
    let mut group_ids = discover_groups(home.clone(), locks.clone())
        .await
        .into_iter()
        .collect::<BTreeSet<_>>();
    group_ids.extend(crate::ops::automation_runtime::pending_delivery_group_ids());
    group_ids.extend(exited.keys().cloned());
    run_groups(home, locks, include_unread, group_ids, exited, cancelled).await;
}

async fn run_groups(
    home: HomeLayout,
    locks: DispatchLocks,
    include_unread: bool,
    group_ids: BTreeSet<String>,
    mut exited: BTreeMap<String, Vec<cccc_runtime::SessionStatus>>,
    cancelled: Arc<AtomicBool>,
) {
    let semaphore = Arc::new(Semaphore::new(GROUP_CONCURRENCY));
    let mut tasks = JoinSet::new();
    for group_id in group_ids {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let group_exits = exited.remove(&group_id).unwrap_or_default();
        let permit = semaphore.clone().acquire_owned().await;
        let Ok(concurrency_permit) = permit else {
            break;
        };
        let home = home.clone();
        let locks = locks.clone();
        let task_cancelled = Arc::clone(&cancelled);
        tasks.spawn(async move {
            let lock = locks.group_write(&group_id).await;
            if task_cancelled.load(Ordering::Acquire) {
                return;
            }
            let _ = tokio::task::spawn_blocking(move || {
                let _lock = lock;
                let _concurrency_permit = concurrency_permit;
                if task_cancelled.load(Ordering::Acquire) {
                    return;
                }
                crate::ops::automation_runtime::maintain_group(&home, &group_id, group_exits);
                if task_cancelled.load(Ordering::Acquire) {
                    return;
                }
                crate::ops::automation_runtime::tick_group(
                    &home,
                    &group_id,
                    include_unread,
                    &task_cancelled,
                );
            })
            .await;
        });
    }
    while tasks.join_next().await.is_some() {}
}

async fn prepare_exited() -> BTreeMap<String, Vec<cccc_runtime::SessionStatus>> {
    tokio::task::spawn_blocking(crate::ops::automation_runtime::prepare_exited)
        .await
        .unwrap_or_default()
}

async fn discover_groups(home: HomeLayout, locks: DispatchLocks) -> Vec<String> {
    let lock = locks.global_read().await;
    tokio::task::spawn_blocking(move || {
        let _lock = lock;
        crate::ops::automation_runtime::group_ids(&home)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::DaemonRequest;
    use cccc_core::GroupStore;
    use serde_json::{Map, json};

    #[test]
    fn unread_work_runs_at_most_once_per_interval() {
        let mut scheduler = AutomationScheduler {
            task: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            last_unread_tick: Instant::now() - AutomationScheduler::UNREAD_INTERVAL,
        };
        assert!(scheduler.take_unread_due());
        assert!(!scheduler.take_unread_due());
    }

    #[tokio::test]
    async fn a_busy_group_does_not_block_global_reads_during_automation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("automation", "")
            .expect("group");
        let locks = DispatchLocks::default();
        let blocked = locks.group_write(&group.group_id).await;
        let task = tokio::spawn(run_groups(
            home,
            locks.clone(),
            false,
            BTreeSet::from([group.group_id]),
            BTreeMap::new(),
            Arc::new(AtomicBool::new(false)),
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;

        let request = DaemonRequest {
            v: 1,
            op: "group_list".into(),
            args: json!({}).as_object().cloned().unwrap_or_else(Map::new),
        };
        assert!(
            tokio::time::timeout(Duration::from_millis(50), locks.acquire(&request))
                .await
                .is_ok(),
            "automation must not queue a global writer behind one busy group"
        );
        drop(blocked);
        task.await.expect("automation task");
    }

    #[tokio::test]
    async fn shutdown_cancels_a_stalled_automation_task() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let mut scheduler = AutomationScheduler {
            task: Some(tokio::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    while !worker_cancelled.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })
                .await;
            })),
            cancelled,
            last_unread_tick: Instant::now(),
        };

        tokio::time::timeout(Duration::from_secs(1), scheduler.finish())
            .await
            .expect("automation shutdown must be bounded");
        assert!(scheduler.task.is_none());
    }
}

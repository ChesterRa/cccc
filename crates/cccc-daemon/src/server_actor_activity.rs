use cccc_core::HomeLayout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct ActorActivityService {
    cancelled: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl ActorActivityService {
    pub fn start(home: HomeLayout) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let task = tokio::spawn(async move {
            let mut publisher = crate::ops::actor_activity::Publisher::default();
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            while !worker_cancelled.load(Ordering::Acquire) {
                interval.tick().await;
                if worker_cancelled.load(Ordering::Acquire) {
                    break;
                }
                let tick_home = home.clone();
                publisher = tokio::task::spawn_blocking(move || {
                    if let Err(error) = publisher.tick(&tick_home) {
                        tracing::warn!(%error, "actor.activity tick failed");
                    }
                    publisher
                })
                .await
                .unwrap_or_default();
            }
        });
        Self { cancelled, task }
    }

    pub async fn finish(self) {
        self.cancelled.store(true, Ordering::Release);
        let mut task = self.task;
        if tokio::time::timeout(Duration::from_millis(500), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}

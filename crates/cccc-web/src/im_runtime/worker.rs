use futures_util::future::select_all;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{AbortHandle, JoinHandle};

const STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) type Stopper = Arc<dyn Fn() + Send + Sync>;

pub(super) struct WorkerHandles {
    task: JoinHandle<()>,
    child_aborts: Vec<AbortHandle>,
    stopper: Stopper,
}

impl WorkerHandles {
    pub(super) fn new(tasks: Vec<JoinHandle<()>>, stopper: Stopper) -> Self {
        let child_aborts = tasks.iter().map(JoinHandle::abort_handle).collect();
        let supervisor_stopper = Arc::clone(&stopper);
        let task = tokio::spawn(async move {
            if tasks.is_empty() {
                return;
            }
            let (_, _, remaining) = select_all(tasks).await;
            supervisor_stopper();
            for task in &remaining {
                task.abort();
            }
            for task in remaining {
                let _ = task.await;
            }
        });
        Self {
            task,
            child_aborts,
            stopper,
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub(super) async fn shutdown(mut self) {
        (self.stopper)();
        if tokio::time::timeout(STOP_TIMEOUT, &mut self.task)
            .await
            .is_err()
        {
            for task in &self.child_aborts {
                task.abort();
            }
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for WorkerHandles {
    fn drop(&mut self) {
        (self.stopper)();
        for task in &self.child_aborts {
            task.abort();
        }
        self.task.abort();
    }
}

pub(super) fn no_op_stopper() -> Stopper {
    Arc::new(|| {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn graceful_shutdown_reaches_child_before_abort_fallback() {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let closed = Arc::new(AtomicBool::new(false));
        let child_closed = Arc::clone(&closed);
        let child = tokio::spawn(async move {
            while !*stop_rx.borrow() {
                if stop_rx.changed().await.is_err() {
                    return;
                }
            }
            child_closed.store(true, Ordering::SeqCst);
        });
        let stopper: Stopper = Arc::new(move || {
            let _ = stop_tx.send(true);
        });

        WorkerHandles::new(vec![child], stopper).shutdown().await;

        assert!(closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn multiple_stalled_workers_shutdown_concurrently() {
        let workers = (0..3)
            .map(|_| {
                WorkerHandles::new(vec![tokio::spawn(std::future::pending())], no_op_stopper())
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();

        futures_util::future::join_all(workers.into_iter().map(WorkerHandles::shutdown)).await;

        assert!(started.elapsed() < STOP_TIMEOUT + Duration::from_secs(1));
    }
}

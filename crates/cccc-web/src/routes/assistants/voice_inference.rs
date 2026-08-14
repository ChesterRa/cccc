use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static NATIVE_INFERENCE: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(super) fn try_acquire() -> Option<OwnedSemaphorePermit> {
    semaphore().try_acquire_owned().ok()
}

pub(super) async fn acquire() -> OwnedSemaphorePermit {
    semaphore()
        .acquire_owned()
        .await
        .expect("native inference semaphore is never closed")
}

fn semaphore() -> Arc<Semaphore> {
    NATIVE_INFERENCE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_inference_is_globally_serialized() {
        let permit = try_acquire().expect("first permit");
        assert!(try_acquire().is_none());
        drop(permit);
        assert!(try_acquire().is_some());
    }

    #[tokio::test]
    async fn queued_inference_waits_instead_of_failing() {
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.clone().acquire_owned().await.expect("first");
        let waiter = tokio::spawn({
            let semaphore = semaphore.clone();
            async move { semaphore.acquire_owned().await.expect("queued") }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(permit);
        let _queued_permit = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter timeout")
            .expect("waiter task");
    }
}

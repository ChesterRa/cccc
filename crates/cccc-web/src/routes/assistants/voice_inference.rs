use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static NATIVE_INFERENCE: OnceLock<Arc<Semaphore>> = OnceLock::new();

pub(super) fn try_acquire() -> Option<OwnedSemaphorePermit> {
    NATIVE_INFERENCE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .ok()
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
}

use std::sync::{Arc, OnceLock};

static CHROME_TEST_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

// Shared by surface, prompt and route tests. Keep the guard until all fixture
// browsers are closed to limit cold-launch contention on small CI runners.
pub(crate) async fn chrome_test_guard() -> tokio::sync::OwnedMutexGuard<()> {
    Arc::clone(CHROME_TEST_LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))))
        .lock_owned()
        .await
}

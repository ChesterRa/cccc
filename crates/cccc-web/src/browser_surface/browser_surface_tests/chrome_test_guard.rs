use std::sync::{Arc, OnceLock};

static CHROME_TEST_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

pub(super) async fn chrome_test_guard() -> tokio::sync::OwnedMutexGuard<()> {
    Arc::clone(CHROME_TEST_LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))))
        .lock_owned()
        .await
}

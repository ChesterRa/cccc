use std::sync::{Arc, OnceLock};

static CHROME_TEST_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

/// Set by PR CI, which leaves real-browser fixtures to the nightly `chrome`
/// nextest profile.
pub(crate) const FORBID_REAL_CHROME_ENV: &str = "CCCC_TEST_FORBID_REAL_CHROME";

// Shared by surface, prompt and route tests. Keep the guard until all fixture
// browsers are closed to limit cold-launch contention on small CI runners.
pub(crate) async fn chrome_test_guard() -> tokio::sync::OwnedMutexGuard<()> {
    assert!(
        std::env::var_os(FORBID_REAL_CHROME_ENV).is_none(),
        "this test launches a real Chrome, which PR CI does not run; add it to the \
         real-Chrome filter in .config/nextest.toml so the nightly job covers it"
    );
    Arc::clone(CHROME_TEST_LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))))
        .lock_owned()
        .await
}

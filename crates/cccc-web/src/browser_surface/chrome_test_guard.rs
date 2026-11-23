use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};

static CHROME_TEST_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

/// Set by PR CI, which leaves real-browser fixtures to the nightly `chrome`
/// nextest profile.
pub(crate) const FORBID_REAL_CHROME_ENV: &str = "CCCC_TEST_FORBID_REAL_CHROME";

/// Set by the nightly `chrome` job. Without it, real-Chrome fixtures skip, so a
/// local `cargo test` or pre-commit run never opens browser windows.
pub(crate) const REAL_CHROME_ENV: &str = "CCCC_TEST_REAL_CHROME";

fn real_chrome_opted_in(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

// Shared by surface, prompt and route tests. Keep the guard until all fixture
// browsers are closed to limit cold-launch contention on small CI runners.
// `None` means the test must return before launching a browser.
pub(crate) async fn chrome_test_guard() -> Option<tokio::sync::OwnedMutexGuard<()>> {
    assert!(
        std::env::var_os(FORBID_REAL_CHROME_ENV).is_none(),
        "this test launches a real Chrome, which PR CI does not run; add it to the \
         real-Chrome filter in .config/nextest.toml so the nightly job covers it"
    );
    if !real_chrome_opted_in(std::env::var_os(REAL_CHROME_ENV).as_deref()) {
        return None;
    }
    Some(
        Arc::clone(CHROME_TEST_LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(()))))
            .lock_owned()
            .await,
    )
}

#[cfg(test)]
mod tests {
    use super::real_chrome_opted_in;
    use std::ffi::OsStr;

    #[test]
    fn real_chrome_fixtures_run_only_when_explicitly_opted_in() {
        assert!(!real_chrome_opted_in(None));
        assert!(!real_chrome_opted_in(Some(OsStr::new(""))));
        assert!(!real_chrome_opted_in(Some(OsStr::new("0"))));
        assert!(real_chrome_opted_in(Some(OsStr::new("1"))));
    }
}

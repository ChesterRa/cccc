//! Validate libtest's executed count as well as the isolated process exit status.
use std::process::Output;

pub(super) fn assert_one_passed(output: Output, filter: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let exactly_one = stdout
        .lines()
        .any(|line| line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored;"));
    assert!(
        output.status.success() && exactly_one,
        "isolated filter '{filter}' must execute exactly one passing test:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[should_panic(expected = "must execute exactly one passing test")]
fn rejects_a_successful_process_that_did_not_run_the_requested_test() {
    let filter = "ops::isolated_test::nonexistent_fixture_test";
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([filter, "--exact", "--nocapture"])
        .output()
        .expect("empty libtest selection");
    assert!(
        output.status.success(),
        "libtest accepts an empty selection"
    );
    assert_one_passed(output, filter);
}

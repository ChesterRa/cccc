use cccc_core::workspace_git::ignored;
use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn large_ignore_query_drains_both_pipes() {
    // Isolate the potentially deadlocked call so the parent can kill and reap it on timeout.
    const CHILD: &str = "CCCC_TEST_LARGE_IGNORE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let repo = tempfile::tempdir().expect("create repository fixture");
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize git repository")
                .success()
        );
        std::fs::write(repo.path().join(".gitignore"), "ignored-*\n").expect("write ignore rules");
        let candidates: Vec<String> = (0..20_000)
            .map(|index| format!("ignored-{index:05}-{}", "x".repeat(80)))
            .chain(std::iter::once("visible.txt".to_owned()))
            .collect();
        let expected: BTreeSet<_> = candidates[..20_000].iter().cloned().collect();
        assert_eq!(ignored(repo.path(), &candidates), expected);
        assert!(ignored(repo.path(), &["visible.txt".into()]).is_empty());
        return;
    }
    let mut child = Command::new(std::env::current_exe().expect("locate test binary"))
        .args([
            "--exact",
            "large_ignore_query_drains_both_pipes",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn isolated ignore query");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll ignore query") {
            assert!(status.success(), "large ignore query failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed-out ignore query");
            child.wait().expect("reap timed-out ignore query");
            panic!("large ignore query deadlocked for 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

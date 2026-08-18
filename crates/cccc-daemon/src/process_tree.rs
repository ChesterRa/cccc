use anyhow::Result;

/// Put the daemon host in an operating-system-owned process container before
/// any actor is restored or launched. On Windows, child processes inherit job
/// membership at creation time, so closing the daemon's last job handle also
/// terminates Codex and every MCP descendant after an abrupt host exit.
#[cfg(windows)]
pub fn protect_daemon_host() -> Result<()> {
    use std::sync::{Mutex, OnceLock};
    use win32job::{ExtendedLimitInfo, Job};

    static ROOT_JOB: OnceLock<Job> = OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());

    if ROOT_JOB.get().is_some() {
        return Ok(());
    }
    let _guard = INIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ROOT_JOB.get().is_some() {
        return Ok(());
    }

    let mut limits = ExtendedLimitInfo::new();
    limits.limit_kill_on_job_close();
    let job = Job::create_with_limit_info(&limits)?;
    job.assign_current_process()?;

    // Once assigned, dropping the only job handle would terminate this daemon.
    // Keep it process-global; Windows closes it automatically at process exit,
    // including TerminateProcess and console-close termination.
    if let Err(job) = ROOT_JOB.set(job) {
        std::mem::forget(job);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn protect_daemon_host() -> Result<()> {
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::protect_daemon_host;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const MODE: &str = "CCCC_WINDOWS_JOB_TEST_MODE";
    const PIDS: &str = "CCCC_WINDOWS_JOB_TEST_PIDS";

    #[test]
    fn abrupt_daemon_exit_reaps_child_and_grandchild_without_deleting_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pid_path = temp.path().join("descendants.pid");
        let ledger_path = temp.path().join("ledger.jsonl");
        let history_path = temp.path().join("historical.pty");
        std::fs::write(&ledger_path, b"durable ledger event\n").expect("ledger marker");
        std::fs::write(&history_path, b"historical terminal bytes").expect("history marker");

        let host = spawn_helper("host_helper", "host", &pid_path);
        let mut host = KillOnDrop::new(host);
        wait_for_file(&pid_path, Duration::from_secs(10));
        let descendants = read_pids(&pid_path);
        assert_eq!(descendants.len(), 2, "child and grandchild pids");
        assert!(descendants.iter().all(|pid| process_exists(*pid)));

        let host_pid = host.id();
        let killed = Command::new("taskkill")
            .args(["/PID", &host_pid.to_string(), "/F"])
            .output()
            .expect("force kill daemon host without /T");
        assert!(
            killed.status.success(),
            "taskkill failed: {}",
            String::from_utf8_lossy(&killed.stderr)
        );
        host.wait().expect("wait for killed host");

        let deadline = Instant::now() + Duration::from_secs(5);
        while descendants.iter().any(|pid| process_exists(*pid)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let survivors = descendants
            .iter()
            .copied()
            .filter(|pid| process_exists(*pid))
            .collect::<Vec<_>>();
        assert!(
            survivors.is_empty(),
            "processes survived daemon host kill: {survivors:?}"
        );
        assert_eq!(
            std::fs::read(&ledger_path).expect("ledger after kill"),
            b"durable ledger event\n"
        );
        assert_eq!(
            std::fs::read(&history_path).expect("history after kill"),
            b"historical terminal bytes"
        );
    }

    #[test]
    fn host_helper() {
        if std::env::var(MODE).as_deref() != Ok("host") {
            return;
        }
        protect_daemon_host().expect("protect daemon host");
        let pid_path = required_path(PIDS);
        let mut child = spawn_helper("child_helper", "child", &pid_path);
        child.wait().expect("wait for child helper");
    }

    #[test]
    fn child_helper() {
        if std::env::var(MODE).as_deref() != Ok("child") {
            return;
        }
        let pid_path = required_path(PIDS);
        let mut grandchild = spawn_helper("grandchild_helper", "grandchild", &pid_path);
        std::fs::write(
            pid_path,
            format!("{}\n{}\n", std::process::id(), grandchild.id()),
        )
        .expect("publish descendant pids");
        grandchild.wait().expect("wait for grandchild helper");
    }

    #[test]
    fn grandchild_helper() {
        if std::env::var(MODE).as_deref() != Ok("grandchild") {
            return;
        }
        std::thread::sleep(Duration::from_secs(300));
    }

    fn spawn_helper(test_name: &str, mode: &str, pid_path: &Path) -> Child {
        Command::new(std::env::current_exe().expect("test executable"))
            .args([
                &format!("process_tree::tests::{test_name}"),
                "--exact",
                "--nocapture",
            ])
            .env(MODE, mode)
            .env(PIDS, pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn process-tree helper")
    }

    fn required_path(name: &str) -> std::path::PathBuf {
        std::env::var_os(name)
            .map(Into::into)
            .unwrap_or_else(|| panic!("missing {name}"))
    }

    fn wait_for_file(path: &Path, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(path.exists(), "{} was not created", path.display());
    }

    fn read_pids(path: &Path) -> Vec<u32> {
        std::fs::read_to_string(path)
            .expect("read pid file")
            .lines()
            .map(|line| line.trim().parse().expect("numeric pid"))
            .collect()
    }

    fn process_exists(pid: u32) -> bool {
        let filter = format!("PID eq {pid}");
        let output = Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist");
        String::from_utf8_lossy(&output.stdout).contains(&format!(",\"{pid}\","))
    }

    struct KillOnDrop {
        child: Child,
    }

    impl KillOnDrop {
        fn new(child: Child) -> Self {
            Self { child }
        }

        fn id(&self) -> u32 {
            self.child.id()
        }

        fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
            self.child.wait()
        }
    }

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .status();
            let _ = self.child.wait();
        }
    }
}

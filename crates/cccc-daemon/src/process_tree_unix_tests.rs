use super::protect_daemon_host;
use cccc_core::HomeLayout;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const MODE: &str = "CCCC_UNIX_GROUP_TEST_MODE";
const HOME: &str = "CCCC_UNIX_GROUP_TEST_HOME";
const PIDS: &str = "CCCC_UNIX_GROUP_TEST_PIDS";

#[test]
fn abrupt_daemon_exit_terminates_its_owned_process_groups() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pid_path = temp.path().join("owned.pid");
    let mut host = spawn_helper("host_helper", "host", temp.path(), &pid_path);
    let owned = wait_for_pid(&pid_path);
    let _cleanup = KillGroupOnDrop(owned);
    assert!(process_exists(owned));

    kill_now(host.id());
    host.wait().expect("reap killed host");

    assert!(
        exits_within(owned, Duration::from_secs(10)),
        "owned process group survived SIGKILL of its daemon"
    );
}

#[test]
fn next_daemon_terminates_groups_its_killed_watchdog_left_behind() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pid_path = temp.path().join("owned.pid");
    let mut host = spawn_helper("host_helper", "host", temp.path(), &pid_path);
    let owned = wait_for_pid(&pid_path);
    let _cleanup = KillGroupOnDrop(owned);
    let watchdog = watchdog_of(host.id());

    kill_now(watchdog);
    kill_now(host.id());
    host.wait().expect("reap killed host");
    assert!(
        !exits_within(owned, Duration::from_secs(3)),
        "the killed watchdog cannot have terminated the group"
    );
    assert!(
        process_exists(owned),
        "the group must survive until the restart"
    );

    let done = temp.path().join("restarted");
    let mut restarted = spawn_helper("restart_helper", "restart", temp.path(), &done);
    restarted.wait().expect("restarted daemon helper");
    assert!(done.exists(), "restarted helper did not finish");
    assert!(
        exits_within(owned, Duration::from_secs(10)),
        "the next daemon left the recorded group running"
    );
}

#[test]
fn a_rejected_second_start_leaves_the_running_daemons_groups_alone() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pid_path = temp.path().join("owned.pid");
    let mut owner = KillOnDrop(spawn_helper(
        "owner_helper",
        "owner",
        temp.path(),
        &pid_path,
    ));
    let owned = wait_for_pid(&pid_path);
    let _cleanup = KillGroupOnDrop(owned);

    let refused = temp.path().join("refused");
    let mut second = spawn_helper("second_start_helper", "second", temp.path(), &refused);
    second.wait().expect("second daemon start");

    assert!(refused.exists(), "the second start was not refused");
    assert!(
        owner.0.try_wait().expect("poll running daemon").is_none(),
        "the running daemon must keep running"
    );
    assert!(
        !exits_within(owned, Duration::from_secs(3)),
        "a refused start terminated the running daemon's process group"
    );
}

#[test]
fn owner_helper() {
    if std::env::var(MODE).as_deref() != Ok("owner") {
        return;
    }
    let home = helper_home();
    home.initialize().expect("initialize home");
    let paths = crate::paths::DaemonPaths::new(home);
    std::fs::create_dir_all(&paths.daemon_dir).expect("daemon dir");
    let _lock = crate::server::claim_home(&paths).expect("claim home");
    let (child, _tree) = cccc_runtime::OwnedProcessTree::spawn(
        Command::new("sleep")
            .arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .expect("spawn owned group");
    std::fs::write(required(PIDS), format!("{}\n", child.id())).expect("publish owned pid");
    std::thread::sleep(Duration::from_secs(300));
}

#[test]
fn second_start_helper() {
    if std::env::var(MODE).as_deref() != Ok("second") {
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let error = runtime
        .block_on(crate::server::run(helper_home()))
        .expect_err("the home is already owned");
    assert!(error.to_string().contains("already owns"), "{error:#}");
    std::fs::write(required(PIDS), b"refused").expect("publish refusal");
}

#[test]
fn host_helper() {
    if std::env::var(MODE).as_deref() != Ok("host") {
        return;
    }
    protect_daemon_host(&helper_home()).expect("protect daemon host");
    let (child, _tree) = cccc_runtime::OwnedProcessTree::spawn(
        Command::new("sleep")
            .arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .expect("spawn owned group");
    std::fs::write(required(PIDS), format!("{}\n", child.id())).expect("publish owned pid");
    std::thread::sleep(Duration::from_secs(300));
}

#[test]
fn restart_helper() {
    if std::env::var(MODE).as_deref() != Ok("restart") {
        return;
    }
    protect_daemon_host(&helper_home()).expect("protect restarted daemon host");
    std::fs::write(required(PIDS), b"done").expect("publish restart");
}

fn spawn_helper(test_name: &str, mode: &str, home: &Path, pids: &Path) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args([
            &format!("process_tree::unix_tests::{test_name}"),
            "--exact",
            "--nocapture",
        ])
        .env(MODE, mode)
        .env(HOME, home.join("home"))
        .env(PIDS, pids)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon host helper")
}

fn helper_home() -> HomeLayout {
    HomeLayout::from_path(required(HOME)).expect("helper home")
}

fn required(name: &str) -> std::path::PathBuf {
    std::env::var_os(name)
        .map(Into::into)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn wait_for_pid(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(pid) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "{} was not published",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The watchdog is the host's `sh` child; the owned group leader is `sleep`.
fn watchdog_of(host: u32) -> u32 {
    let output = Command::new("pgrep")
        .args(["-P", &host.to_string(), "-x", "sh"])
        .output()
        .expect("pgrep");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("exactly one watchdog")
}

fn kill_now(pid: u32) {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGKILL,
    )
    .expect("SIGKILL");
}

/// A terminated child of a still-running host stays a zombie until reaped, and a
/// zombie still answers signal 0. Only a process that can still run counts.
fn process_exists(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

fn exits_within(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Stops a helper daemon host even when an assertion fails first.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Leaves nothing behind when an assertion fails; observation never signals.
struct KillGroupOnDrop(i32);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(self.0),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

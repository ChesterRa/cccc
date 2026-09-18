//! Identity of an Agent View worker tree and cleanup of workers whose daemon
//! is gone.
//!
//! Agent View owns its workers: CCCC stops a session through the daemon's
//! control socket and never signals the worker itself. When that daemon exits
//! or is replaced, the worker (and the MCP servers it spawned) keeps running
//! with nothing left to address it, and a resume simply starts a fresh worker.
//!
//! The daemon's job listing reports the pid of the pty host (`--bg-pty-host`),
//! whose only child is the worker (`--bg-spare`). The receipt therefore keeps
//! that host pid plus its process start time, so a resume against an
//! unreachable daemon can reap exactly that host and its worker.

use super::{command, control};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerIdentity {
    /// Pid of the pty host that owns the worker, as reported by Agent View.
    pub(crate) pid: u32,
    /// Process start time as reported by the OS; guards against pid reuse.
    pub(crate) started: String,
}

/// Captures the identity of a live worker tree, or `None` where it cannot be
/// verified later (unsupported platform, process already gone).
pub(crate) fn identify(pid: u32) -> Option<WorkerIdentity> {
    let started = process_start(pid)?;
    Some(WorkerIdentity { pid, started })
}

/// Reaps `worker` when the Agent View daemon for `environment` cannot be
/// reached. Returns `Ok(None)` when the daemon answers (the worker is still
/// managed), `Ok(Some(killed))` after an attempt against an unreachable daemon.
pub(crate) fn reap_unreachable(
    environment: &BTreeMap<String, String>,
    worker: &WorkerIdentity,
) -> io::Result<Option<bool>> {
    let config_dir = command::config_dir(environment)?;
    if daemon_reachable(&config_dir) {
        return Ok(None);
    }
    Ok(Some(reap(worker)))
}

fn daemon_reachable(config_dir: &Path) -> bool {
    match control::Endpoint::resolve(config_dir) {
        Ok(endpoint) => endpoint.reachable(),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn reap(worker: &WorkerIdentity) -> bool {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    if process_start(worker.pid).as_deref() != Some(worker.started.as_str()) {
        return false;
    }
    let Some(arguments) = process_arguments(worker.pid) else {
        return false;
    };
    if !(arguments.contains("claude") && arguments.contains("--bg-pty-host")) {
        return false;
    }
    // The worker is the host's child. Kill it explicitly: a host that dies
    // only closes the pty, and the worker may ignore the resulting SIGHUP.
    let workers = process_children(worker.pid)
        .into_iter()
        .filter(|child| {
            process_arguments(*child).is_some_and(|arguments| arguments.contains("--bg-spare"))
        })
        .collect::<Vec<_>>();
    for child in &workers {
        let _ = kill(Pid::from_raw(*child as i32), Signal::SIGKILL);
    }
    let killed = kill(Pid::from_raw(worker.pid as i32), Signal::SIGKILL).is_ok();
    tracing::warn!(
        host = worker.pid,
        workers = ?workers,
        killed,
        "reaped a Claude Agent View worker whose daemon is unreachable"
    );
    killed
}

#[cfg(not(unix))]
fn reap(_worker: &WorkerIdentity) -> bool {
    false
}

#[cfg(unix)]
fn ps_field(pid: u32, field: &str) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", &format!("{field}="), "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(unix)]
fn process_start(pid: u32) -> Option<String> {
    ps_field(pid, "lstart")
}

#[cfg(not(unix))]
fn process_start(_pid: u32) -> Option<String> {
    None
}

#[cfg(unix)]
fn process_arguments(pid: u32) -> Option<String> {
    ps_field(pid, "args")
}

#[cfg(unix)]
fn process_children(pid: u32) -> Vec<u32> {
    let Ok(output) = std::process::Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Spawns a fake pty host whose child is a fake worker, mirroring the Agent
/// View process tree, and returns the host together with the worker pid.
#[cfg(all(test, unix))]
pub(crate) fn spawn_fake_host() -> (std::process::Child, u32) {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let host = Command::new("bash")
        .args([
            "-c",
            r#"exec -a "claude --bg-pty-host fake" bash -c '( exec -a "claude --bg-spare fake" sleep 30 ) & wait'"#,
        ])
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn fake host");
    let deadline = Instant::now() + Duration::from_secs(5);
    let worker = loop {
        let worker = process_children(host.id()).into_iter().find(|child| {
            process_arguments(*child).is_some_and(|arguments| arguments.contains("--bg-spare"))
        });
        if let Some(worker) = worker {
            break worker;
        }
        assert!(Instant::now() < deadline, "fake worker never appeared");
        std::thread::sleep(Duration::from_millis(20));
    };
    (host, worker)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Child, Command};
    use std::time::Duration;

    fn alive(child: &mut Child) -> bool {
        std::thread::sleep(Duration::from_millis(200));
        child.try_wait().expect("poll").is_none()
    }

    fn pid_alive(pid: u32) -> bool {
        // A killed worker is reparented and reaped once its host is gone;
        // until then it may linger as a zombie, which ps still lists.
        process_arguments(pid).is_some_and(|arguments| arguments.contains("--bg-spare"))
    }

    fn unreachable_environment() -> (tempfile::TempDir, BTreeMap<String, String>) {
        let temp = tempfile::tempdir().expect("tempdir");
        let environment = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            temp.path().to_string_lossy().into_owned(),
        )]);
        (temp, environment)
    }

    #[test]
    fn reaps_the_recorded_host_and_its_worker() {
        let (_temp, environment) = unreachable_environment();
        let (mut host, worker) = spawn_fake_host();
        let identity = identify(host.id()).expect("identity");
        assert!(!identity.started.is_empty());

        assert_eq!(
            reap_unreachable(&environment, &identity).expect("reap"),
            Some(true)
        );
        assert!(!alive(&mut host), "the recorded host must be gone");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!pid_alive(worker), "the host's worker must be gone");
    }

    #[test]
    fn a_reused_pid_or_a_foreign_process_is_left_alone() {
        let (_temp, environment) = unreachable_environment();
        let (mut host, worker) = spawn_fake_host();
        let stale = WorkerIdentity {
            pid: host.id(),
            started: "Thu Jan  1 00:00:00 1970".into(),
        };
        assert_eq!(
            reap_unreachable(&environment, &stale).expect("reap"),
            Some(false)
        );
        assert!(
            alive(&mut host),
            "a different start time means a reused pid"
        );
        assert!(pid_alive(worker), "the worker of a reused pid is untouched");

        let mut bystander = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn bystander");
        let identity = identify(bystander.id()).expect("identity");
        assert_eq!(
            reap_unreachable(&environment, &identity).expect("reap"),
            Some(false)
        );
        assert!(alive(&mut bystander), "only Agent View hosts are reaped");

        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(worker as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
        let _ = host.kill();
        let _ = bystander.kill();
    }
}

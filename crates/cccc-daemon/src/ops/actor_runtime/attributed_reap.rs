//! Reaping processes that outlived the session that spawned them.
//!
//! A session stop kills the process tree the runtime owns (see
//! `crates/cccc-runtime/src/process_tree`). A child that detached into its own
//! session — `setsid`, or a provider CLI that daemonises itself — is no longer
//! part of that tree and survives the stop, so deleting a group can leave live
//! processes behind that still carry the group's `CCCC_GROUP_ID`.
//!
//! Every process cccc launches is branded with `CCCC_GROUP_ID` and
//! `CCCC_ACTOR_ID`, which makes those survivors attributable even after they
//! leave the tree: sweep the process table for the group's tag and stop them.

use std::collections::HashMap;
use std::time::Duration;

#[cfg(target_os = "linux")]
use nix::errno::Errno;
#[cfg(target_os = "linux")]
use nix::sys::signal::{Signal, kill};
#[cfg(target_os = "linux")]
use nix::unistd::Pid;

/// A process the sweep signalled because it still carried the group's tag.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Reaped {
    pub pid: u32,
    pub actor_id: String,
    /// False when the process had already exited or refused the signal.
    pub killed: bool,
}

const TERM_GRACE: Duration = Duration::from_millis(1500);
const TERM_POLL: Duration = Duration::from_millis(100);

/// Stops every process of this user that still carries `CCCC_GROUP_ID=<group_id>`.
///
/// Used after a group's sessions have been stopped, so it only sees what the
/// process-tree kill could not reach. Empty on platforms without a readable
/// process environment (`/proc`).
#[cfg(target_os = "linux")]
pub fn reap_group(group_id: &str) -> Vec<Reaped> {
    let Some(uid) = self_uid() else {
        return Vec::new();
    };
    let mut reaped = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return reaped;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // Skip ourselves: the daemon is not an Actor process.
        if pid == std::process::id() {
            continue;
        }
        // An unreadable environ means another user's process (or one already
        // gone); never signal what we cannot read and do not own.
        let Some(environ) = read_environ(pid) else {
            continue;
        };
        if environ.get("CCCC_GROUP_ID").map(String::as_str) != Some(group_id) {
            continue;
        }
        if process_uid(pid) != Some(uid) {
            continue;
        }
        let actor_id = environ.get("CCCC_ACTOR_ID").cloned().unwrap_or_default();
        let killed = signal(pid, Signal::SIGTERM);
        reaped.push(Reaped {
            pid,
            actor_id,
            killed,
        });
    }
    if reaped.is_empty() {
        return reaped;
    }
    // Give the well-behaved ones a moment to run their shutdown, then insist.
    let deadline = std::time::Instant::now() + TERM_GRACE;
    let mut alive = reaped.iter().map(|entry| entry.pid).collect::<Vec<_>>();
    while std::time::Instant::now() < deadline {
        alive.retain(|pid| process_alive(*pid));
        if alive.is_empty() {
            return reaped;
        }
        std::thread::sleep(TERM_POLL);
    }
    for pid in alive {
        signal(pid, Signal::SIGKILL);
    }
    reaped
}

/// No portable process-environment scan: nothing to sweep off Linux.
#[cfg(not(target_os = "linux"))]
pub fn reap_group(_group_id: &str) -> Vec<Reaped> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn read_environ(pid: u32) -> Option<HashMap<String, String>> {
    let raw = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let mut values = HashMap::new();
    for entry in raw.split(|byte| *byte == 0) {
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        if let Some((key, value)) = entry.split_once('=') {
            values.insert(key.to_string(), value.to_string());
        }
    }
    Some(values)
}

#[cfg(target_os = "linux")]
fn self_uid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    process_uid_from_status(&status)
}

#[cfg(target_os = "linux")]
fn process_uid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    process_uid_from_status(&status)
}

#[cfg(target_os = "linux")]
fn process_uid_from_status(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|uid| uid.split_whitespace().next())
        .and_then(|uid| uid.parse::<u32>().ok())
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    // `kill(pid, None)` reports whether the pid exists at all; a zombie still
    // answers, so also check its state.
    match signal_none(pid) {
        Ok(()) => std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|stat| !stat.contains(" Z "))
            .unwrap_or(false),
        Err(Errno::ESRCH) => false,
        Err(_) => true,
    }
}

#[cfg(target_os = "linux")]
fn signal_none(pid: u32) -> Result<(), Errno> {
    kill(Pid::from_raw(pid as i32), None::<Signal>)
}

#[cfg(target_os = "linux")]
fn signal(pid: u32, signal: Signal) -> bool {
    match kill(Pid::from_raw(pid as i32), signal) {
        Ok(()) => true,
        // Already gone, or we may not signal it: nothing to report either way.
        Err(_) => false,
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn spawn_detached(group_id: &str) -> Option<u32> {
        // `setsid` puts the child in its own session, which is exactly the
        // shape a process-tree kill cannot reach.
        let command = Command::new("setsid")
            .arg("sleep")
            .arg("30")
            .env("CCCC_GROUP_ID", group_id)
            .env("CCCC_ACTOR_ID", "detached-test")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let pid = command.id();
        std::mem::forget(command);
        Some(pid)
    }

    fn wait_for_tag(pid: u32, group_id: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if read_environ(pid)
                .and_then(|env| env.get("CCCC_GROUP_ID").cloned())
                .as_deref()
                == Some(group_id)
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn reaps_a_process_that_left_the_session_tree() {
        let group_id = format!("g_reap_test_{}", std::process::id());
        let Some(pid) = spawn_detached(&group_id) else {
            eprintln!("setsid unavailable; skipping");
            return;
        };
        assert!(wait_for_tag(pid, &group_id), "child never carried the tag");

        let reaped = reap_group(&group_id);

        assert!(
            reaped.iter().any(|entry| entry.pid == pid),
            "detached child {pid} was not reaped: {reaped:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !process_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("detached child {pid} survived the reap");
    }

    #[test]
    fn leaves_other_groups_alone() {
        let group_id = format!("g_reap_test_keep_{}", std::process::id());
        let other = format!("g_reap_test_skip_{}", std::process::id());
        let Some(pid) = spawn_detached(&group_id) else {
            eprintln!("setsid unavailable; skipping");
            return;
        };
        assert!(wait_for_tag(pid, &group_id), "child never carried the tag");

        let reaped = reap_group(&other);

        assert!(reaped.is_empty(), "sweep took {reaped:?} for {other}");
        assert!(process_alive(pid), "child {pid} died on an unrelated sweep");
        let _ = signal(pid, Signal::SIGKILL);
    }
}

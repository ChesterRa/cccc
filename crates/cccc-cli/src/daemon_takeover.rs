//! Lock-based daemon discovery and operator-approved takeover.

use anyhow::{Result, bail};
use cccc_core::HomeLayout;
use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;

const GRACEFUL_TERMINATION: Duration = Duration::from_secs(10);
const FORCED_TERMINATION: Duration = Duration::from_secs(5);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Whether any process currently owns the daemon lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    /// No daemon holds the lock, so no daemon is running for this home.
    Free,
    /// A daemon holds the lock. Whether it can serve is a separate question.
    Held,
}

pub(crate) fn lock_path(home: &HomeLayout) -> PathBuf {
    home.daemon_dir().join("ccccd.lock")
}

/// Probe the daemon lock without keeping it.
pub(crate) fn probe_lock(home: &HomeLayout) -> Result<LockState> {
    let path = lock_path(home);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            anyhow::anyhow!("could not open daemon lock {}: {error}", path.display())
        })?;
    match lock.try_lock_exclusive() {
        Ok(()) => {
            FileExt::unlock(&lock).map_err(|error| {
                anyhow::anyhow!(
                    "could not release daemon lock probe {}: {error}",
                    path.display()
                )
            })?;
            Ok(LockState::Free)
        }
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(LockState::Held)
        }
        Err(error) => bail!("could not probe daemon lock {}: {error}", path.display()),
    }
}

pub(crate) async fn wait_for_lock_release(home: &HomeLayout, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if probe_lock(home)? == LockState::Free {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "CCCC daemon did not release {} within {} seconds",
                lock_path(home).display(),
                timeout.as_secs()
            );
        }
        tokio::time::sleep(LOCK_POLL_INTERVAL).await;
    }
}

/// The message shown when a daemon holds the lock but will not answer.
///
/// It names the uncertainty on purpose. The launcher does not know whether the
/// daemon is busy or wedged, and an operator who is told it is broken cannot
/// weigh the cost of stopping a daemon that is merely slow -- one that may be
/// mid-delivery for actors they care about.
fn unresponsive_daemon_report(home: &HomeLayout, pid: Option<u32>) -> String {
    let process = pid.map_or_else(|| "unknown PID".into(), |pid| format!("PID {pid}"));
    format!(
        "A CCCC daemon ({process}) holds {} but is not answering.\nIt may be busy, or it may be stuck.",
        lock_path(home).display()
    )
}

/// Ask the operator whether to take the daemon lock from an unresponsive
/// daemon, and do it if they agree.
///
/// Declining -- including the automatic decline when nothing can be asked --
/// is reported as an error naming the daemon, because the caller cannot start
/// its own daemon while this one holds the lock.
pub(crate) async fn confirm_and_take_over(home: &HomeLayout) -> Result<()> {
    let pid = daemon_pid(home);
    let report = unresponsive_daemon_report(home, pid);
    if !crate::confirm::ask(&format!("{report}\nStop it and continue?"))? {
        bail!(
            "{report}\nStop it with `cccc daemon stop`, or run cccc from a terminal to take over interactively."
        );
    }
    let Some(pid) = pid else {
        // Nothing published a PID, so there is no process to signal. The lock
        // may still be on its way out from a daemon mid-exit.
        return wait_for_lock_release(home, GRACEFUL_TERMINATION).await;
    };
    if terminate(pid)? {
        if wait_for_lock_release(home, GRACEFUL_TERMINATION)
            .await
            .is_ok()
        {
            return Ok(());
        }
        eprintln!("[cccc] Daemon PID {pid} did not stop when asked; killing it");
    }
    kill(pid)?;
    wait_for_lock_release(home, FORCED_TERMINATION).await
}

/// The PID of the daemon that owns this home, when it published one and the
/// process is still alive and still a CCCC daemon host.
///
/// Every check here keeps a takeover from signalling an unrelated process: a
/// daemon killed hard leaves its PID file behind, and the operating system is
/// free to hand that number to anything.
pub(crate) fn daemon_pid(home: &HomeLayout) -> Option<u32> {
    let pid = std::fs::read_to_string(home.daemon_dir().join("ccccd.pid"))
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    (pid != std::process::id()
        && command_line_of(pid).is_some_and(|command| command_line_is_cccc_daemon_host(&command)))
    .then_some(pid)
}

#[cfg(unix)]
fn command_line_of(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(windows)]
fn command_line_of(pid: u32) -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine"),
        ])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether a command line belongs to a process that hosts the CCCC daemon.
///
/// On Unix the daemon is embedded in the Web host, so a bare `cccc` or
/// `cccc web ...` owns the lock just as `cccc daemon run` does; root flags
/// before the subcommand still name the Web host. Anything else -- an `mcp`
/// bridge, a one-shot subcommand, or a process that merely mentions cccc --
/// must never be a takeover target, because the PID file may be stale.
fn command_line_is_cccc_daemon_host(command: &str) -> bool {
    let command = command.trim();
    let (executable, rest) = match command.strip_prefix('"') {
        Some(quoted) => match quoted.split_once('"') {
            Some(parts) => parts,
            None => return false,
        },
        None => command
            .split_once(char::is_whitespace)
            .unwrap_or((command, "")),
    };
    // The PID file may name a Windows path on a Windows daemon, so neither
    // platform's separator is trusted to `Path` alone.
    let basename = executable.rsplit(['/', '\\']).next().unwrap_or_default();
    let is_cccc = Path::new(basename)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("cccc"));
    if !is_cccc {
        return false;
    }
    use clap::Parser;
    let Ok(args) = shell_words::split(rest) else {
        return false;
    };
    let Ok(cli) = crate::args::Cli::try_parse_from(std::iter::once("cccc".to_owned()).chain(args))
    else {
        return false;
    };
    matches!(
        cli.command,
        None | Some(crate::args::CommandKind::Web(_))
            | Some(crate::args::CommandKind::Daemon {
                action: crate::args::DaemonAction::Run
            })
    )
}

/// Send a cooperative stop. Returns whether one was sent at all: a platform
/// without such a signal reports `false` so the caller skips straight to the
/// forced stop instead of waiting for a request nobody received.
#[cfg(unix)]
fn terminate(pid: u32) -> Result<bool> {
    signal(pid, nix::sys::signal::Signal::SIGTERM).map(|()| true)
}

#[cfg(unix)]
fn kill(pid: u32) -> Result<()> {
    signal(pid, nix::sys::signal::Signal::SIGKILL)
}

#[cfg(unix)]
fn signal(pid: u32, signal: nix::sys::signal::Signal) -> Result<()> {
    let raw =
        i32::try_from(pid).map_err(|_| anyhow::anyhow!("daemon PID {pid} is out of range"))?;
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(raw), signal) {
        // The daemon exited between the liveness check and the signal, which is
        // the outcome a takeover wanted anyway.
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => bail!("could not signal CCCC daemon PID {pid}: {error}"),
    }
}

/// Windows has no cooperative stop signal for a detached process without a
/// shared console, and the shutdown RPC was already attempted by the caller.
#[cfg(windows)]
fn terminate(_pid: u32) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn kill(pid: u32) -> Result<()> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|error| anyhow::anyhow!("could not run taskkill for PID {pid}: {error}"))?;
    if !status.success() {
        bail!("taskkill could not stop CCCC daemon PID {pid}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "daemon_takeover_tests.rs"]
mod tests;

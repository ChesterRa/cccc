//! Unix owned process groups must not outlive the process that owns them, however it
//! exits. Graceful shutdown and `force_terminate_owned` both need the owner to run;
//! SIGKILL, a crash or an abrupt host exit skip them, and Unix has no portable
//! parent-death signal. Two layers cover that:
//!
//! - A watchdog shell in its own process group keeps the read end of a pipe that
//!   carries no data. The kernel closes the owner's write end when the owner dies
//!   for any reason, so the watchdog reads EOF, then terminates the groups in the
//!   owner's latest watch list on disk if that list is fresh enough to trust.
//! - A durable ledger of the same set. The next owner start terminates groups the
//!   watchdog could not, such as after the watchdog itself was killed.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A process group this process owns: its leader's PID and spawn time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OwnedGroup {
    pub(super) pgid: i32,
    pub(super) spawned_at: u64,
}

/// An owned group, and the latest time its owner can have died holding it: a live
/// owner rewrites the entry at least every `HEARTBEAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct LedgerEntry {
    pub(super) pgid: i32,
    pub(super) spawned_at: u64,
    pub(super) alive_until: u64,
}

/// One process in a `ps` snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProcessRow {
    pub(super) pid: i32,
    pub(super) pgid: i32,
    pub(super) started_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Proven to be the recorded group: terminate it.
    Terminate,
    /// Members survive but cannot be proven to be ours: keep the record, signal nothing.
    Retain,
    /// The recorded group no longer exists.
    Forget,
}

/// `ps` reports elapsed time in whole seconds, and spawning takes a moment.
const TIME_TOLERANCE_SECS: u64 = 3;
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
/// A live owner refreshes its entries this often, bounding when it can have died.
const HEARTBEAT: Duration = Duration::from_secs(10);

/// A live owner rewrites its watch list at least every `HEARTBEAT`. An older list
/// means the watchdog resumed long after the owner died, when its PGIDs may have
/// been reused; the next owner's verified reconciliation handles it instead.
const WATCH_LIST_MAX_AGE: Duration = Duration::from_secs(3 * HEARTBEAT.as_secs());

/// Wait for the owner's death, then terminate the groups in its fresh watch list
/// (`$1`, holding `written_at pgid...`) after a grace period.
const WATCHDOG: &str = r#"trap '' INT HUP
cat >/dev/null
read -r written groups < "$1" || exit 0
[ -n "$groups" ] || exit 0
[ $(( $(date +%s) - written )) -le "$2" ] || exit 0
for group in $groups; do kill -s TERM -- "-$group" 2>/dev/null; done
sleep 2
for group in $groups; do kill -s KILL -- "-$group" 2>/dev/null; done
exit 0
"#;

struct Guard {
    ledger: PathBuf,
    watch_list: PathBuf,
    /// Held open, never written: its closing is the owner's death notice.
    watchdog: ChildStdin,
    groups: Vec<OwnedGroup>,
    /// A previous owner's surviving groups that could not be proven to be its own.
    retained: Vec<LedgerEntry>,
}

fn guard() -> MutexGuard<'static, Option<Guard>> {
    static GUARD: OnceLock<Mutex<Option<Guard>>> = OnceLock::new();
    GUARD
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Terminate what a previous owner using `ledger` left behind, then record this
/// process's groups there. A process that already guards another ledger (a daemon
/// restarted for another home) moves to it with a new watchdog. Returns false when
/// `ledger` is already the current one.
pub(super) fn enable(ledger: &Path) -> io::Result<bool> {
    let mut guard = guard();
    if guard.as_ref().is_some_and(|guard| guard.ledger == ledger) {
        return Ok(false);
    }
    if let Some(directory) = ledger.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let retained = reconcile(&read_ledger(ledger));
    let watch_list = instance_watch_list(ledger);
    write_watch_list(&watch_list, &[])?;
    // Reconciliation above already handled every earlier instance's groups. Their
    // lists go too, so a watchdog that reads late finds nothing to act on.
    remove_other_watch_lists(ledger, &watch_list);
    let watchdog = spawn_watchdog(&watch_list)?;
    let current = match guard.as_mut() {
        Some(current) => {
            // Empty the old list first: closing the old pipe wakes its watchdog.
            // A removed old home leaves no list, which that watchdog also ignores.
            let _ = write_watch_list(&current.watch_list, &[]);
            current.ledger = ledger.to_path_buf();
            current.watch_list = watch_list;
            current.watchdog = watchdog;
            current.retained = retained;
            current
        }
        None => {
            std::thread::Builder::new()
                .name("cccc-owned-groups-heartbeat".into())
                .spawn(heartbeat)?;
            guard.insert(Guard {
                ledger: ledger.to_path_buf(),
                watch_list,
                watchdog,
                groups: Vec::new(),
                retained,
            })
        }
    };
    persist(current)?;
    Ok(true)
}

/// Publish the complete owned set. Called with the process registry lock held,
/// which forced exit also needs: only bounded local file writes happen here, and
/// the watchdog reads the result from disk, so none of it waits on the watchdog.
pub(super) fn publish(groups: &[OwnedGroup]) -> io::Result<()> {
    let mut guard = guard();
    let Some(guard) = guard.as_mut() else {
        return Ok(());
    };
    guard.groups = groups.to_vec();
    persist(guard)
}

/// Keep `alive_until` ahead of now for groups that outlive their last publication.
fn heartbeat() {
    loop {
        std::thread::sleep(HEARTBEAT);
        if let Some(guard) = guard().as_ref()
            && let Err(error) = persist(guard)
        {
            eprintln!("could not refresh owned process groups: {error}");
        }
    }
}

fn persist(guard: &Guard) -> io::Result<()> {
    // One missed refresh of slack for scheduling delays.
    let alive_until = now() + 2 * HEARTBEAT.as_secs();
    let entries = guard
        .groups
        .iter()
        .map(|group| LedgerEntry {
            pgid: group.pgid,
            spawned_at: group.spawned_at,
            alive_until,
        })
        .chain(guard.retained.iter().copied())
        .collect::<Vec<_>>();
    // Retained groups are unproven, so only the owned set reaches the watchdog.
    write_watch_list(&guard.watch_list, &guard.groups)?;
    write_atomically(&guard.ledger, &serde_json::to_vec(&entries)?)
}

/// Each instance's watchdog reads only its own list. A shared list would let a
/// watchdog that reads late terminate groups of the instance that replaced it.
fn instance_watch_list(ledger: &Path) -> PathBuf {
    let instance = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let mut name = watch_list_prefix(ledger);
    name.push_str(&format!("{}-{instance}.watchdog", std::process::id()));
    ledger.with_file_name(name)
}

fn watch_list_prefix(ledger: &Path) -> String {
    let stem = ledger
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    format!("{stem}.")
}

fn remove_other_watch_lists(ledger: &Path, keep: &Path) {
    let (Some(directory), prefix) = (ledger.parent(), watch_list_prefix(ledger)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let is_watch_list = name.starts_with(&prefix)
            && (name.ends_with(".watchdog") || name.ends_with(".watchdog.tmp"));
        if is_watch_list && path != keep {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_watch_list(path: &Path, groups: &[OwnedGroup]) -> io::Result<()> {
    let mut line = now().to_string();
    for group in groups {
        line.push(' ');
        line.push_str(&group.pgid.to_string());
    }
    line.push('\n');
    write_atomically(path, line.as_bytes())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}

fn watchdog_command(watch_list: &Path) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", WATCHDOG, "cccc-watchdog"])
        .arg(watch_list)
        .arg(WATCH_LIST_MAX_AGE.as_secs().to_string());
    command
}

fn spawn_watchdog(watch_list: &Path) -> io::Result<ChildStdin> {
    use std::os::unix::process::CommandExt;
    let mut watchdog = watchdog_command(watch_list)
        // Terminal signals to the owner's process group must not reach it.
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Its exit is reaped by init once the owner is gone; it never exits first.
    watchdog
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("watchdog has no stdin"))
}

fn read_ledger(ledger: &Path) -> Vec<LedgerEntry> {
    // A missing or unreadable ledger names nothing that can be safely signalled.
    std::fs::read(ledger)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Terminate every entry proven to be the recorded group; return those to retain.
pub(super) fn reconcile(entries: &[LedgerEntry]) -> Vec<LedgerEntry> {
    if entries.is_empty() {
        return Vec::new();
    }
    let Some(rows) = process_rows() else {
        // Without a snapshot nothing can be proven or ruled out.
        return entries.to_vec();
    };
    let mut terminate = Vec::new();
    let mut retained = Vec::new();
    for entry in entries {
        match classify(entry, &rows) {
            Verdict::Terminate => terminate.push(entry.pgid),
            Verdict::Retain => retained.push(*entry),
            Verdict::Forget => {}
        }
    }
    terminate_groups(&terminate);
    retained
}

/// No PID is reused while a process group with that ID exists. So a live leader
/// with another start time means the recorded group ended, and a member that
/// started before its owner can have died can only belong to it. Survivors that
/// all started later cannot be told from a group that reused the PID afterwards.
pub(super) fn classify(entry: &LedgerEntry, rows: &[ProcessRow]) -> Verdict {
    let members = rows
        .iter()
        .filter(|row| row.pgid == entry.pgid)
        .collect::<Vec<_>>();
    if members.is_empty() {
        return Verdict::Forget;
    }
    if let Some(leader) = members.iter().find(|row| row.pid == entry.pgid) {
        return if leader.started_at.abs_diff(entry.spawned_at) <= TIME_TOLERANCE_SECS {
            Verdict::Terminate
        } else {
            Verdict::Forget
        };
    }
    let owned_window = entry.spawned_at.saturating_sub(TIME_TOLERANCE_SECS)
        ..=entry.alive_until.saturating_add(TIME_TOLERANCE_SECS);
    if members
        .iter()
        .any(|row| owned_window.contains(&row.started_at))
    {
        Verdict::Terminate
    } else {
        Verdict::Retain
    }
}

fn process_rows() -> Option<Vec<ProcessRow>> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,pgid=,etime="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let now = now();
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some(ProcessRow {
                    pid: fields.next()?.parse().ok()?,
                    pgid: fields.next()?.parse().ok()?,
                    started_at: now.saturating_sub(parse_elapsed(fields.next()?)?),
                })
            })
            .collect(),
    )
}

fn terminate_groups(groups: &[i32]) {
    for pgid in groups {
        signal(*pgid, Some(nix::sys::signal::Signal::SIGTERM));
    }
    let deadline = Instant::now() + TERMINATE_GRACE;
    while groups.iter().any(|pgid| alive(*pgid)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pgid in groups.iter().copied().filter(|pgid| alive(*pgid)) {
        signal(pgid, Some(nix::sys::signal::Signal::SIGKILL));
    }
}

/// `ps -o etime` is `[[dd-]hh:]mm:ss`.
pub(super) fn parse_elapsed(value: &str) -> Option<u64> {
    let (days, clock) = match value.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, value),
    };
    let mut seconds = 0;
    for part in clock.split(':') {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(days * 86_400 + seconds)
}

fn alive(pgid: i32) -> bool {
    signal(pgid, None)
}

fn signal(pgid: i32, signal: Option<nix::sys::signal::Signal>) -> bool {
    nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), signal).is_ok()
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;

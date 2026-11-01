//! Unix owned process groups must not outlive the process that owns them, however it
//! exits. Graceful shutdown and `force_terminate_owned` both need the owner to run;
//! SIGKILL, a crash or an abrupt host exit skip them, and Unix has no portable
//! parent-death signal. Two layers cover that:
//!
//! - A watchdog shell in its own process group keeps the read end of a pipe that
//!   carries no data. The kernel closes the owner's write end when the owner dies
//!   for any reason, so the watchdog reads EOF, then terminates the groups in the
//!   owner's latest watch list on disk after checking freshness and process identity.
//! - A durable ledger of the same set. The next owner start terminates groups the
//!   watchdog could not, such as after the watchdog itself was killed.

use super::guard_record::{
    CapturedIdentity, GroupEvidence, GroupRecord, IdentityStrength, LaunchIntent, Ledger, Origin,
    Record, UnavailableReason,
};
use crate::process_recovery::{
    RecoveryDiagnostic, RecoveryError, RecoveryRecord, RecoverySnapshot, RetryResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(target_os = "linux")]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A process group this process owns: its leader's PID and spawn time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnedGroup {
    pub(super) pgid: i32,
    pub(super) spawned_at: u64,
    pub(super) id: uuid::Uuid,
    pub(super) origin: Option<Origin>,
    pub(super) evidence: GroupEvidence,
}

/// An owned group and the last time its owner actually confirmed holding it.
/// Never extend that observation into a future heartbeat interval: a replacement
/// group might already exist during that interval after an abrupt exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct LedgerEntry {
    pub(super) pgid: i32,
    pub(super) spawned_at: u64,
    pub(super) observed_at: u64,
}

impl<'de> Deserialize<'de> for LedgerEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct StoredEntry {
            pgid: i32,
            spawned_at: u64,
            observed_at: Option<u64>,
            alive_until: Option<u64>,
        }
        let stored = StoredEntry::deserialize(deserializer)?;
        let observed_at = stored
            .observed_at
            .or_else(|| {
                // The original on-disk format added exactly 20 seconds to its
                // observation. Remove that allowance; never grant it authority.
                stored.alive_until.map(|until| until.saturating_sub(20))
            })
            .ok_or_else(|| serde::de::Error::custom("missing process ownership observation"))?;
        Ok(Self {
            pgid: stored.pgid,
            spawned_at: stored.spawned_at,
            observed_at,
        })
    }
}

/// `ps` reports elapsed time in whole seconds, and spawning takes a moment.
const TIME_TOLERANCE_SECS: u64 = 3;
#[cfg(target_os = "linux")]
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
/// A live owner refreshes its last confirmed ownership observation this often.
const HEARTBEAT: Duration = Duration::from_secs(10);

/// A live owner rewrites its watch list at least every `HEARTBEAT`. An older list
/// means the watchdog resumed long after the owner died, when its PGIDs may have
/// been reused; the next owner's verified reconciliation handles it instead.
const WATCH_LIST_MAX_AGE: Duration = Duration::from_secs(3 * HEARTBEAT.as_secs());

/// Wait for the owner's death, then check the identities in its fresh watch list
/// (`$1`, holding `written_at pgid:spawned_at...`) before each termination signal.
/// Keep the ps identity predicate aligned with `classify` below.
#[cfg(not(target_os = "linux"))]
const WATCHDOG: &str = r#"trap '' INT HUP
cat >/dev/null
read -r written groups < "$1" || exit 0
[ -n "$groups" ] || exit 0
tolerance=$3
age=$(( $(date +%s) - written ))
[ "$age" -ge 0 ] && [ "$age" -le "$2" ] || exit 0
owned_groups() {
  ps -A -o pid=,pgid=,etime= | awk -v groups="$groups" -v observed="$written" -v now="$(date +%s)" -v tolerance="$tolerance" '
    BEGIN {
      count = split(groups, records, " ")
      for (i = 1; i <= count; i++) {
        if (split(records[i], pair, ":") == 2 && pair[1] ~ /^[0-9]+$/ && pair[1] > 1 && pair[2] ~ /^[0-9]+$/)
          born[pair[1]] = pair[2]
      }
    }
    NF == 3 && $1 ~ /^[0-9]+$/ && $2 in born && $3 ~ /^[0-9:-]+$/ {
      days = 0; clock = $3
      if (split(clock, day, "-") == 2) { days = day[1]; clock = day[2] }
      count = split(clock, parts, ":"); elapsed = 0
      for (i = 1; i <= count; i++) elapsed = elapsed * 60 + parts[i]
      started = now - days * 86400 - elapsed
      if ($1 == $2) {
        leader[$2] = 1
        delta = started - born[$2]
        matching[$2] = delta >= -tolerance && delta <= tolerance
      }
      if (started >= born[$2] - tolerance && started <= observed - tolerance) member[$2] = 1
    }
    END { for (group in born) if (leader[group] ? matching[group] : member[group]) print group }
  '
}
for group in $(owned_groups); do kill -s TERM -- "-$group" 2>/dev/null; done
sleep 2
for group in $(owned_groups); do kill -s KILL -- "-$group" 2>/dev/null; done
exit 0
"#;

/// Linux watchdog: compare the recorded scope and exact `/proc` start tick.
/// Numeric group signalling still has a check-to-signal race. A missing or
/// unverified procfs view, scope or leader causes no signal.
#[cfg(target_os = "linux")]
const WATCHDOG: &str = r#"trap '' INT HUP
set -f
cat >/dev/null
read -r written version groups < "$1" || exit 0
[ "$version" = v2 ] || exit 0
[ -n "$groups" ] || exit 0
age=$(( $(date +%s) - written ))
[ "$age" -ge 0 ] && [ "$age" -le "$2" ] || exit 0

# The current directory pins the procfs mount for every subsequent relative
# read, including the helpers' own /proc/self references.
cd /proc || exit 0
[ "$(stat -f -c %t . 2>/dev/null)" = 9fa0 ] || exit 0
awk '/^NSpid:/ { found++; if (NF != 2 || $2 !~ /^[0-9]+$/ || $2 == 0) bad=1 }
     END { exit !(found == 1 && !bad) }' ./self/status 2>/dev/null || exit 0
boot=$(cat ./sys/kernel/random/boot_id 2>/dev/null) || exit 0
pidns=$(readlink ./self/ns/pid 2>/dev/null) || exit 0
case "$pidns" in pid:\[*\]) pidns=${pidns#pid:[}; pidns=${pidns%]} ;; *) exit 0 ;; esac
case "$pidns" in ''|*[!0-9]*) exit 0 ;; esac
nsnames=$(ls ./self/ns 2>/dev/null) || exit 0
if printf '%s\n' "$nsnames" | grep -qx time; then
  timens=$(readlink ./self/ns/time 2>/dev/null) || exit 0
  case "$timens" in time:\[*\]) timens=${timens#time:[}; timens=${timens%]} ;; *) exit 0 ;; esac
  case "$timens" in ''|*[!0-9]*) exit 0 ;; esac
else
  timens=-
fi

owned_groups() {
  for record in $groups; do
    previous_ifs=$IFS; IFS=:; set -- $record; IFS=$previous_ifs
    [ "$#" -eq 8 ] || continue
    group=$1; start=$3; recorded_boot=$4; recorded_pidns=$5; recorded_timens=$6
    case "$group" in ''|*[!0-9]*) continue ;; esac
    case "$start" in ''|*[!0-9]*) continue ;; esac
    [ "$group" -gt 1 ] 2>/dev/null || continue
    [ "$recorded_boot" = "$boot" ] && [ "$recorded_pidns" = "$pidns" ] &&
      [ "$recorded_timens" = "$timens" ] || continue
    line=$(cat "./$group/stat" 2>/dev/null) || continue
    case "$line" in "$group "*) ;; *) continue ;; esac
    # Fields after the final ')' begin with state; field 3 is PGID and field
    # 20 is starttime. Malformed or split command names fail the checks.
    rest=${line##*) }
    set -- $rest
    [ "$#" -ge 20 ] || continue
    [ "$3" = "$group" ] || continue
    shift 19
    [ "$1" = "$start" ] || continue
    printf '%s\n' "$group"
  done
}
term_groups=$(owned_groups)
[ -n "$term_groups" ] || exit 0
for group in $term_groups; do kill -s TERM -- "-$group" 2>/dev/null; done
sleep 2
for group in $(owned_groups); do kill -s KILL -- "-$group" 2>/dev/null; done
exit 0
"#;

struct Guard {
    ledger: PathBuf,
    watch_list: PathBuf,
    /// Held open, never written: its closing is the owner's death notice.
    watchdog: ChildStdin,
    groups: Vec<OwnedGroup>,
    /// A previous owner's surviving groups that could not be proven to be its own.
    retained: Vec<Record>,
    /// Launches for this owner, persisted before invoking a child process.
    intents: Vec<InFlightIntent>,
    /// False after any incomplete publication or source-removal failure.
    /// Acceptance remains disabled until a later owner absorbs every source.
    sources_clean: bool,
    retrying: std::collections::HashSet<uuid::Uuid>,
}

fn guard() -> MutexGuard<'static, Option<Guard>> {
    static GUARD: OnceLock<Mutex<Option<Guard>>> = OnceLock::new();
    GUARD
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn offline_lock() -> MutexGuard<'static, ()> {
    static OFFLINE: OnceLock<Mutex<()>> = OnceLock::new();
    OFFLINE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

struct InFlightIntent {
    entry: LaunchIntent,
    abandoned: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub(super) struct PreparedIntent {
    pub(super) entry: LaunchIntent,
    /// The initial tripwire passed. The caller still needs the documented
    /// whole-window no-external-reaper/no-auto-reap precondition.
    pub(super) may_capture_identity: bool,
    abandoned: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for PreparedIntent {
    fn drop(&mut self) {
        // The spawn path may unwind on an async worker. Marking the already
        // persisted intent must not take the guard mutex or perform disk I/O.
        self.abandoned
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Guard {
    fn has_abandoned_intent(&self) -> bool {
        self.intents
            .iter()
            .any(|intent| intent.abandoned.load(std::sync::atomic::Ordering::Acquire))
    }

    fn promote_abandoned_intents(&mut self) {
        self.intents.retain(|intent| {
            if intent.abandoned.load(std::sync::atomic::Ordering::Acquire) {
                self.retained
                    .push(Record::LaunchIntent(intent.entry.clone()));
                false
            } else {
                true
            }
        });
    }
}

/// Record a protected spawn before starting it. The registry lock must already
/// be held, establishing the same registry-then-guard order as publication.
pub(super) fn begin_intent() -> io::Result<Option<PreparedIntent>> {
    let mut current = guard();
    let Some(current) = current.as_mut() else {
        return Ok(None);
    };
    let created_at = now();
    if created_at == 0 {
        return Err(io::Error::other("cannot timestamp protected launch"));
    }
    #[cfg(target_os = "linux")]
    let (origin, may_capture_identity) = match super::linux_identity::ProcView::open() {
        Ok(view) => {
            let origin = view.origin().ok();
            let allowed = origin.is_some() && view.check_child_disposition().is_ok();
            (origin, allowed)
        }
        Err(_) => (None, false),
    };
    #[cfg(not(target_os = "linux"))]
    let (origin, may_capture_identity) = (None, false);
    let entry = LaunchIntent {
        id: uuid::Uuid::new_v4(),
        source_id: None,
        created_at,
        origin,
    };
    let abandoned = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    current.intents.push(InFlightIntent {
        entry: entry.clone(),
        abandoned: abandoned.clone(),
    });
    if let Err(error) = persist(current) {
        current.intents.pop();
        return Err(error);
    }
    Ok(Some(PreparedIntent {
        entry,
        may_capture_identity,
        abandoned,
    }))
}

/// Remove an intent only when the process API reports that it never started.
/// On a persistence error put the intent back so a heartbeat cannot erase it.
pub(super) fn cancel_unstarted_intent(id: uuid::Uuid) -> io::Result<()> {
    let mut current = guard();
    let current = current
        .as_mut()
        .ok_or_else(|| io::Error::other("protected launch guard is unavailable"))?;
    let Some(index) = current
        .intents
        .iter()
        .position(|intent| intent.entry.id == id)
    else {
        return Err(io::Error::other("protected launch intent is missing"));
    };
    let removed = current.intents.remove(index);
    if let Err(error) = persist(current) {
        current.intents.insert(index, removed);
        return Err(error);
    }
    Ok(())
}

/// A launch that may have run is a recovery concern even while this owner is
/// still alive. Keep it visible in memory if a subsequent write also fails.
pub(super) fn retain_uncertain_intent(id: uuid::Uuid) -> io::Result<()> {
    let mut current = guard();
    let current = current
        .as_mut()
        .ok_or_else(|| io::Error::other("protected launch guard is unavailable"))?;
    let Some(index) = current
        .intents
        .iter()
        .position(|intent| intent.entry.id == id)
    else {
        return Err(io::Error::other("protected launch intent is missing"));
    };
    let entry = current.intents.remove(index);
    current.retained.push(Record::LaunchIntent(entry.entry));
    persist(current)
}

/// Atomically replace this owner's in-flight intent with its registered group.
/// An error restores the intent in memory; the caller must treat the child as
/// already started and attempt its owned cleanup.
pub(super) fn publish_started(groups: &[OwnedGroup], id: Option<uuid::Uuid>) -> io::Result<()> {
    let mut current = guard();
    let Some(current) = current.as_mut() else {
        return if id.is_none() {
            Ok(())
        } else {
            Err(io::Error::other("protected launch guard is unavailable"))
        };
    };
    let Some(id) = id else {
        return Err(io::Error::other("protected launch has no intent"));
    };
    let Some(index) = current
        .intents
        .iter()
        .position(|intent| intent.entry.id == id)
    else {
        return Err(io::Error::other("protected launch intent is missing"));
    };
    let removed = current.intents.remove(index);
    let previous_groups = std::mem::replace(&mut current.groups, groups.to_vec());
    if let Err(error) = persist(current) {
        current.groups = previous_groups;
        current.intents.insert(index, removed);
        return Err(error);
    }
    Ok(())
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub(super) fn recovery_pending() -> bool {
    guard().as_ref().is_some_and(|guard| {
        !guard.retained.is_empty() || guard.has_abandoned_intent() || !guard.sources_clean
    })
}

pub(crate) fn active_for(ledger: &Path) -> bool {
    guard().as_ref().is_some_and(|guard| guard.ledger == ledger)
}

pub(super) fn has_active_guard() -> bool {
    guard().is_some()
}

/// Reconcile an old, separately owned ledger without replacing this process's
/// current guard. The caller must hold the old owner's exclusive instance lock.
pub(super) fn recover_ledger(ledger: &Path) -> io::Result<()> {
    let _offline = offline_lock();
    if active_for(ledger) {
        return Err(io::Error::other("owned process guard is already active"));
    }
    let (mut records, ledger_temps) = read_ledger_sources(ledger).map_err(|error| {
        let name = ledger
            .file_name()
            .map(|name| name.to_string_lossy().escape_debug().to_string())
            .unwrap_or_else(|| "<unknown>".to_owned());
        io::Error::new(
            error.kind(),
            format!("cannot read Web process ledger {name}: {:?}", error.kind()),
        )
    })?;
    // The watchdog list is persisted before the ledger. Include it even if
    // the ledger is absent or older, including an interrupted atomic write.
    let (watch_entries, mut watch_paths) = read_recovery_watch_lists(ledger)?;
    watch_paths.extend(ledger_temps);
    records.extend(watch_entries);
    normalize_source_records(&mut records)?;
    let retained = reconcile_records(records);
    if retained.is_empty() {
        // Do not let an old watchdog wake later and act on a replacement.
        // Failure to delete any list keeps recovery pending.
        for path in watch_paths {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        match std::fs::remove_file(ledger) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        return Ok(());
    }
    write_ledger_atomically(ledger, &Ledger::new(retained.clone()).to_disk_bytes()?)?;
    remove_watch_paths(&watch_paths)?;
    report_retained(&retained);
    Err(io::Error::other(
        "owned process recovery is not yet confirmed",
    ))
}

fn read_recovery_watch_lists(ledger: &Path) -> io::Result<(Vec<Record>, Vec<PathBuf>)> {
    let Some(directory) = ledger.parent() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let directory = match std::fs::read_dir(directory) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => return Err(error),
    };
    let prefix = watch_list_prefix(ledger);
    let mut files = directory.collect::<io::Result<Vec<_>>>()?;
    files.sort_by_key(std::fs::DirEntry::file_name);
    let mut entries = Vec::new();
    let mut paths = Vec::new();
    for entry in files {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix)
            || !(name.ends_with(".watchdog")
                || name.ends_with(".watchdog.tmp")
                || name.contains(".watchdog.tmp."))
        {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "guard watch source is not a regular file",
            ));
        }
        let path = entry.path();
        let text = File::open(&path)
            .and_then(read_ledger_bytes)
            .and_then(|bytes| {
                String::from_utf8(bytes).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid guard watch encoding")
                })
            })
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "cannot read Web process watch list {}: {:?}",
                        name.escape_debug(),
                        error.kind()
                    ),
                )
            })?;
        entries.extend(
            parse_watch_text(&text, entry.file_name().as_bytes()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid Web process watch list {}", name.escape_debug()),
                )
            })?,
        );
        paths.push(path);
    }
    Ok((entries, paths))
}

fn parse_watch_text(text: &str, source: &[u8]) -> io::Result<Vec<Record>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid guard watch source");
    // A partial write can contain parseable numeric prefixes; the writer adds
    // a newline only after constructing the complete list.
    if !text.ends_with('\n') {
        return Err(invalid());
    }
    let mut fields = text.split_whitespace();
    let observed_at: u64 = fields
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    if observed_at == 0 {
        return Err(invalid());
    }
    let mut fields = fields.peekable();
    if fields.peek() == Some(&"v2") {
        fields.next();
        fields
            .map(|field| parse_v2_watch_entry(field, observed_at))
            .collect()
    } else {
        let mut entries = Vec::new();
        for field in fields {
            let (pgid, spawned_at) = field.split_once(':').ok_or_else(invalid)?;
            entries.push(LedgerEntry {
                pgid: pgid.parse().map_err(|_| invalid())?,
                spawned_at: spawned_at.parse().map_err(|_| invalid())?,
                observed_at,
            });
        }
        legacy_records(entries, source)
    }
}

fn parse_v2_watch_entry(field: &str, observed_at: u64) -> io::Result<Record> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid guard watch entry");
    let fields = field.split(':').collect::<Vec<_>>();
    if fields.len() != 8 {
        return Err(invalid());
    }
    let pgid = fields[0].parse().map_err(|_| invalid())?;
    let spawned_at = fields[1].parse().map_err(|_| invalid())?;
    let start_ticks = fields[2].parse().map_err(|_| invalid())?;
    let pid_namespace = fields[4].parse().map_err(|_| invalid())?;
    let time_namespace = match fields[5] {
        "-" => None,
        value => Some(value.parse().map_err(|_| invalid())?),
    };
    let id = uuid::Uuid::parse_str(fields[6]).map_err(|_| invalid())?;
    let strength = match fields[7] {
        "a1" => IdentityStrength::Approximate {
            reason: UnavailableReason::PidfdUnavailable,
        },
        "a2" => IdentityStrength::Approximate {
            reason: UnavailableReason::PidfdDenied,
        },
        "a3" => IdentityStrength::Approximate {
            reason: UnavailableReason::LegacyPidfd,
        },
        value => IdentityStrength::Pidfs {
            inode: value
                .strip_prefix('p')
                .and_then(|inode| inode.parse().ok())
                .ok_or_else(invalid)?,
        },
    };
    let origin = Origin {
        boot_id: fields[3].to_owned(),
        pid_namespace,
        time_namespace,
    };
    let record = Record::ProcessGroup(GroupRecord {
        id,
        source_id: None,
        pgid,
        spawned_at,
        observed_at,
        origin: Some(origin.clone()),
        evidence: GroupEvidence::Captured(CapturedIdentity {
            origin,
            start_ticks,
            strength,
        }),
    });
    if !record.valid() {
        return Err(invalid());
    }
    Ok(record)
}

/// Retry only previously retained groups, never the current owner's live set.
pub(super) fn refresh_active(ledger: &Path) -> io::Result<()> {
    {
        let current = guard();
        if current
            .as_ref()
            .is_none_or(|current| current.ledger != ledger)
        {
            return Err(io::Error::other("owned process guard is not active"));
        }
    }
    reconcile_active(ledger, None).map_err(io::Error::other)?;
    let current = guard();
    let current = current
        .as_ref()
        .filter(|current| current.ledger == ledger)
        .ok_or_else(|| io::Error::other("owned process guard is not active"))?;
    if current.retained.is_empty() && !current.has_abandoned_intent() && current.sources_clean {
        Ok(())
    } else {
        report_retained(&current.retained);
        Err(io::Error::other(
            "owned process recovery is not yet confirmed",
        ))
    }
}

fn record_version(record: &Record) -> Result<String, RecoveryError> {
    let bytes = serde_json::to_vec(record).map_err(|_| RecoveryError::Unavailable)?;
    let hash = Sha256::digest(bytes);
    Ok(hash.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn current_origin() -> Option<Origin> {
    #[cfg(target_os = "linux")]
    {
        super::linux_identity::ProcView::open()
            .ok()
            .and_then(|view| view.origin().ok())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn view_record(
    record: &Record,
    current: Option<&Origin>,
    sources_clean: bool,
    retrying: bool,
) -> Result<RecoveryRecord, RecoveryError> {
    let (kind, created, origin, evidence) = match record {
        Record::LaunchIntent(intent) => (
            "launch_intent",
            intent.created_at,
            intent.origin.as_ref(),
            None,
        ),
        Record::ProcessGroup(group) => (
            "process_group",
            group.spawned_at,
            group.origin.as_ref(),
            Some(&group.evidence),
        ),
    };
    let origin_label = match (origin, current) {
        (Some(previous), Some(current)) if previous.boot_id != current.boot_id => "previous_boot",
        (Some(previous), Some(current)) if previous != current => "other_namespace",
        (Some(_), Some(_)) => "same_origin",
        _ => "unknown",
    };
    let reason = match (record, evidence, origin_label) {
        (Record::LaunchIntent(_), _, _) => "launch_incomplete",
        (_, Some(GroupEvidence::Legacy), _) => "legacy_identity",
        (_, Some(GroupEvidence::Unconfirmed), _) => "identity_unconfirmed",
        (_, _, "other_namespace") => "foreign_namespace",
        (_, _, "unknown") => "origin_unknown",
        _ => "cleanup_unconfirmed",
    };
    Ok(RecoveryRecord {
        id: record.id().to_string(),
        version: record_version(record)?,
        kind,
        created_at_unix_seconds: Some(created),
        origin: origin_label,
        reason,
        retryable: !retrying,
        acceptance_supported: sources_clean && !retrying,
    })
}

pub(crate) fn list_active(ledger: &Path) -> Result<RecoverySnapshot, RecoveryError> {
    let (records, sources_clean, retrying) = {
        let mut current = guard();
        let current = current
            .as_mut()
            .filter(|current| current.ledger == ledger)
            .ok_or(RecoveryError::Unavailable)?;
        current.promote_abandoned_intents();
        (
            current.retained.clone(),
            current.sources_clean,
            !current.retrying.is_empty(),
        )
    };
    snapshot(records, sources_clean, retrying)
}

fn snapshot(
    records: Vec<Record>,
    sources_clean: bool,
    retrying: bool,
) -> Result<RecoverySnapshot, RecoveryError> {
    let origin = current_origin();
    let records = records
        .iter()
        .map(|record| view_record(record, origin.as_ref(), sources_clean, retrying))
        .collect::<Result<Vec<_>, _>>()?;
    let diagnostics = (!sources_clean)
        .then_some(RecoveryDiagnostic {
            reason: "other",
            hint: "use_offline_review",
            relative_path: None,
        })
        .into_iter()
        .collect();
    Ok(RecoverySnapshot {
        records,
        diagnostics,
    })
}

fn locked_sources(ledger: &Path) -> io::Result<(Vec<Record>, Vec<PathBuf>)> {
    let (mut records, mut sources) = read_ledger_sources(ledger)?;
    let (watches, watch_paths) = read_recovery_watch_lists(ledger)?;
    records.extend(watches);
    sources.extend(watch_paths);
    normalize_source_records(&mut records)?;
    Ok((records, sources))
}

fn diagnostic_snapshot(error: io::Error, relative_path: Option<String>) -> RecoverySnapshot {
    let (reason, hint) = match error.kind() {
        io::ErrorKind::InvalidData => ("evidence_invalid", "use_offline_review"),
        io::ErrorKind::PermissionDenied => ("evidence_unreadable", "inspect_permissions"),
        _ => ("evidence_unreadable", "inspect_storage"),
    };
    RecoverySnapshot {
        records: Vec::new(),
        diagnostics: vec![RecoveryDiagnostic {
            reason,
            hint,
            relative_path,
        }],
    }
}

// Only standard owner directories and conservative file names are exposed.
// Arbitrary external paths and source contents remain private.
fn diagnostic_path(ledger: &Path, name: Option<&std::ffi::OsStr>) -> Option<String> {
    let parent = ledger.parent()?.file_name()?.to_str()?;
    if !matches!(parent, "daemon" | "web-processes") {
        return None;
    }
    let Some(name) = name else {
        return Some(parent.to_owned());
    };
    let name = name.to_str()?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return None;
    }
    Some(format!("{parent}/{name}"))
}

struct OpaqueSource {
    path: PathBuf,
    view: RecoveryRecord,
}

/// Inventory one owner's recognized files without exposing their contents.
/// A readable but malformed artifact can be reviewed as a whole; unreadable
/// or non-regular sources remain diagnostics and cannot be accepted.
fn inspect_locked_sources(
    ledger: &Path,
) -> io::Result<(Vec<Record>, Vec<OpaqueSource>, Vec<RecoveryDiagnostic>)> {
    let directory = ledger
        .parent()
        .ok_or_else(|| io::Error::other("missing ledger directory"))?;
    let ledger_name = ledger
        .file_name()
        .ok_or_else(|| io::Error::other("missing ledger name"))?;
    let ledger_text = ledger_name.to_string_lossy();
    let temporary_prefix = format!("{ledger_text}.tmp");
    let watch_prefix = watch_list_prefix(ledger);
    let files = match std::fs::read_dir(directory) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new(), Vec::new()));
        }
        Err(error) => return Err(error),
    };
    let mut files = files.collect::<io::Result<Vec<_>>>()?;
    files.sort_by_key(std::fs::DirEntry::file_name);
    let mut records = Vec::new();
    let mut opaque = Vec::new();
    let mut diagnostics = Vec::new();
    for file in files {
        let name = file.file_name();
        let text = name.to_string_lossy();
        let ledger_source = name == ledger_name
            || text == temporary_prefix
            || text.starts_with(&format!("{temporary_prefix}."));
        let watch_source = text.starts_with(&watch_prefix)
            && (text.ends_with(".watchdog")
                || text.ends_with(".watchdog.tmp")
                || text.contains(".watchdog.tmp."));
        if !ledger_source && !watch_source {
            continue;
        }
        let bytes = match file.file_type() {
            Ok(kind) if kind.is_file() => File::open(file.path()).and_then(read_ledger_bytes),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a regular file",
            )),
            Err(error) => Err(error),
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                diagnostics.extend(
                    diagnostic_snapshot(error, diagnostic_path(ledger, Some(&name))).diagnostics,
                );
                continue;
            }
        };
        let parsed = if ledger_source {
            parse_ledger_records(&bytes, name.as_bytes())
        } else {
            std::str::from_utf8(&bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid watch encoding"))
                .and_then(|text| parse_watch_text(text, name.as_bytes()))
        };
        match parsed {
            Ok(items) => records.extend(items),
            Err(_) => {
                let version_hash = Sha256::digest(&bytes);
                let version = version_hash
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                let mut digest = Sha256::new();
                digest.update(b"cccc-opaque-process-evidence-v1\0");
                digest.update(name.as_bytes());
                let hash = digest.finalize();
                let mut id = [0; 16];
                id.copy_from_slice(&hash[..16]);
                id[6] = (id[6] & 0x0f) | 0x80;
                id[8] = (id[8] & 0x3f) | 0x80;
                opaque.push(OpaqueSource {
                    path: file.path(),
                    view: RecoveryRecord {
                        id: uuid::Uuid::from_bytes(id).to_string(),
                        version,
                        kind: "opaque_evidence",
                        created_at_unix_seconds: None,
                        origin: "unknown",
                        reason: "evidence_invalid",
                        retryable: false,
                        acceptance_supported: true,
                    },
                });
            }
        }
    }
    normalize_source_records(&mut records)?;
    Ok((records, opaque, diagnostics))
}

pub(crate) fn list_locked_ledger(ledger: &Path) -> Result<RecoverySnapshot, RecoveryError> {
    let _offline = offline_lock();
    if active_for(ledger) {
        return Err(RecoveryError::Unavailable);
    }
    match inspect_locked_sources(ledger) {
        Ok((records, opaque, diagnostics)) => {
            let normal_acceptance = opaque.is_empty() && diagnostics.is_empty();
            let mut result = snapshot(records, normal_acceptance, false)?;
            result.diagnostics = diagnostics;
            result
                .records
                .extend(opaque.into_iter().map(|source| source.view));
            Ok(result)
        }
        Err(error) => Ok(diagnostic_snapshot(error, diagnostic_path(ledger, None))),
    }
}

fn persist_locked_sources(
    ledger: &Path,
    records: Vec<Record>,
    sources: &[PathBuf],
) -> Result<(), RecoveryError> {
    let document = Ledger::new(records);
    let bytes = document
        .to_disk_bytes()
        .map_err(|_| RecoveryError::Unavailable)?;
    write_ledger_atomically(ledger, &bytes).map_err(|_| RecoveryError::Unavailable)?;
    remove_watch_paths(sources).map_err(|_| RecoveryError::Unavailable)
}

pub(crate) fn retry_locked_ledger(
    ledger: &Path,
    id: &str,
    version: &str,
) -> Result<RetryResult, RecoveryError> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| RecoveryError::Changed)?;
    let _offline = offline_lock();
    if active_for(ledger) {
        return Err(RecoveryError::Unavailable);
    }
    let (_, opaque, diagnostics) =
        inspect_locked_sources(ledger).map_err(|_| RecoveryError::Unavailable)?;
    if let Some(source) = opaque
        .iter()
        .find(|source| source.view.id == id.to_string())
    {
        if source.view.version != version {
            return Err(RecoveryError::Changed);
        }
        return Ok(RetryResult::StillPending);
    }
    if !opaque.is_empty() || !diagnostics.is_empty() {
        return Err(RecoveryError::Unavailable);
    }
    let (mut records, sources) = locked_sources(ledger).map_err(|_| RecoveryError::Unavailable)?;
    let index = records
        .iter()
        .position(|record| record.id() == id)
        .ok_or(RecoveryError::Changed)?;
    if record_version(&records[index])? != version {
        return Err(RecoveryError::Changed);
    }
    let remaining = reconcile_records(vec![records[index].clone()]);
    let result = if remaining.is_empty() {
        records.remove(index);
        RetryResult::Cleared
    } else {
        RetryResult::StillPending
    };
    persist_locked_sources(ledger, records, &sources)?;
    Ok(result)
}

pub(crate) fn accept_locked_ledger(
    ledger: &Path,
    id: &str,
    version: &str,
) -> Result<(), RecoveryError> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| RecoveryError::Changed)?;
    let _offline = offline_lock();
    if active_for(ledger) {
        return Err(RecoveryError::Unavailable);
    }
    let (_, opaque, diagnostics) =
        inspect_locked_sources(ledger).map_err(|_| RecoveryError::Unavailable)?;
    if let Some(source) = opaque
        .iter()
        .find(|source| source.view.id == id.to_string())
    {
        if source.view.version != version {
            return Err(RecoveryError::Changed);
        }
        // The exact readable artifact was reviewed as potentially containing
        // multiple unknown records. Other source files remain untouched.
        return std::fs::remove_file(&source.path).map_err(|_| RecoveryError::Unavailable);
    }
    if !opaque.is_empty() || !diagnostics.is_empty() {
        return Err(RecoveryError::Unavailable);
    }
    let (mut records, sources) = locked_sources(ledger).map_err(|_| RecoveryError::Unavailable)?;
    let index = records
        .iter()
        .position(|record| record.id() == id)
        .ok_or(RecoveryError::Changed)?;
    if record_version(&records[index])? != version {
        return Err(RecoveryError::Changed);
    }
    // First persist all source evidence and remove every absorbed source. If
    // any step fails, the reviewed record is still present and not accepted.
    if !sources.is_empty() {
        persist_locked_sources(ledger, records.clone(), &sources)?;
    }
    records.remove(index);
    persist_locked_sources(ledger, records, &[])
}

struct RetryReservation {
    ledger: PathBuf,
    ids: Vec<uuid::Uuid>,
}

impl Drop for RetryReservation {
    fn drop(&mut self) {
        let mut current = guard();
        if let Some(current) = current
            .as_mut()
            .filter(|current| current.ledger == self.ledger)
        {
            for id in &self.ids {
                current.retrying.remove(id);
            }
        }
    }
}

/// Reserve evidence under the guard mutex, do slow checks without that mutex,
/// then apply only the exact records reserved. A cancelled async caller does
/// not cancel the owning blocking operation.
fn reconcile_active(
    ledger: &Path,
    wanted: Option<(uuid::Uuid, &str)>,
) -> Result<bool, RecoveryError> {
    let (selected, reservation) = {
        let mut current = guard();
        let current = current
            .as_mut()
            .filter(|current| current.ledger == ledger)
            .ok_or(RecoveryError::Unavailable)?;
        current.promote_abandoned_intents();
        if !current.retrying.is_empty() {
            return Err(RecoveryError::Unavailable);
        }
        let selected = if let Some((id, version)) = wanted {
            let record = current
                .retained
                .iter()
                .find(|record| record.id() == id)
                .ok_or(RecoveryError::Changed)?;
            if record_version(record)? != version {
                return Err(RecoveryError::Changed);
            }
            vec![record.clone()]
        } else {
            current.retained.clone()
        };
        if selected.is_empty() {
            return Ok(true);
        }
        let ids = selected.iter().map(Record::id).collect::<Vec<_>>();
        current.retrying.extend(ids.iter().copied());
        (
            selected,
            RetryReservation {
                ledger: current.ledger.clone(),
                ids,
            },
        )
    };
    let remaining = reconcile_records(selected.clone());
    let remaining_ids = remaining
        .iter()
        .map(Record::id)
        .collect::<std::collections::HashSet<_>>();
    let selected_ids = reservation
        .ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let cleared = remaining.is_empty();
    {
        let mut current = guard();
        let current = current
            .as_mut()
            .filter(|current| current.ledger == reservation.ledger)
            .ok_or(RecoveryError::Unavailable)?;
        if selected
            .iter()
            .any(|original| !current.retained.iter().any(|record| record == original))
        {
            return Err(RecoveryError::Changed);
        }
        let before = current.retained.clone();
        current.retained.retain(|record| {
            !selected_ids.contains(&record.id()) || remaining_ids.contains(&record.id())
        });
        if current.retained != before && persist(current).is_err() {
            current.retained = before;
            return Err(RecoveryError::Unavailable);
        }
    }
    drop(reservation);
    Ok(cleared)
}

pub(crate) fn retry_active(
    ledger: &Path,
    id: &str,
    version: &str,
) -> Result<RetryResult, RecoveryError> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| RecoveryError::Changed)?;
    if reconcile_active(ledger, Some((id, version)))? {
        Ok(RetryResult::Cleared)
    } else {
        Ok(RetryResult::StillPending)
    }
}

pub(crate) fn accept_active(ledger: &Path, id: &str, version: &str) -> Result<(), RecoveryError> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| RecoveryError::Changed)?;
    let mut current = guard();
    let current = current
        .as_mut()
        .filter(|current| current.ledger == ledger)
        .ok_or(RecoveryError::Unavailable)?;
    current.promote_abandoned_intents();
    if !current.sources_clean || !current.retrying.is_empty() {
        return Err(RecoveryError::Unavailable);
    }
    let index = current
        .retained
        .iter()
        .position(|record| record.id() == id)
        .ok_or(RecoveryError::Changed)?;
    if record_version(&current.retained[index])? != version {
        return Err(RecoveryError::Changed);
    }
    let removed = current.retained.remove(index);
    if persist(current).is_err() {
        current.retained.insert(index, removed);
        return Err(RecoveryError::Unavailable);
    }
    Ok(())
}

fn report_retained(entries: &[Record]) {
    for entry in entries {
        match entry {
            Record::ProcessGroup(group) => crate::diagnostic(format_args!(
                "owned process recovery pending: pgid={} started_at={}",
                group.pgid, group.spawned_at
            )),
            Record::LaunchIntent(intent) => crate::diagnostic(format_args!(
                "owned process launch outcome is unknown: started_at={}",
                intent.created_at
            )),
        }
    }
}

/// Terminate what a previous owner using `ledger` left behind, then record this
/// process's groups there. A process that already guards another ledger (a daemon
/// restarted for another home) moves to it with a new watchdog. Returns false when
/// `ledger` is already the current one.
pub(super) fn enable(ledger: &Path) -> io::Result<bool> {
    let _offline = offline_lock();
    let mut guard = guard();
    if guard.as_ref().is_some_and(|guard| guard.ledger == ledger) {
        return Ok(false);
    }
    if guard.as_ref().is_some_and(|current| {
        !current.groups.is_empty()
            || !current.retained.is_empty()
            || !current.intents.is_empty()
            || !current.retrying.is_empty()
    }) {
        return Err(io::Error::other(
            "cannot change process guard while ownership remains unresolved",
        ));
    }
    if let Some(directory) = ledger.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let (mut records, ledger_temps) = read_ledger_sources(ledger)?;
    let (watch_entries, mut watch_paths) = read_recovery_watch_lists(ledger)?;
    watch_paths.extend(ledger_temps);
    records.extend(watch_entries);
    normalize_source_records(&mut records)?;
    let retained = reconcile_records(records);
    let watch_list = instance_watch_list(ledger);
    write_watch_list(&watch_list, &[])?;
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
            current.intents.clear();
            current.sources_clean = false;
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
                intents: Vec::new(),
                sources_clean: false,
                retrying: std::collections::HashSet::new(),
            })
        }
    };
    persist(current)?;
    // Only discard source watches once all retained evidence has reached the
    // ledger. A crash or write error before this point leaves the sources.
    remove_watch_paths(&watch_paths)?;
    current.sources_clean = true;
    Ok(true)
}

/// Publish the complete owned set. Called with the process registry lock held,
/// which forced exit also needs. Local file I/O here can block on the filesystem;
/// the watchdog reads the result from disk, so publication does not wait on it.
pub(super) fn publish(groups: &[OwnedGroup]) -> io::Result<()> {
    let mut guard = guard();
    let Some(guard) = guard.as_mut() else {
        return Ok(());
    };
    guard.groups = groups.to_vec();
    persist(guard)
}

/// Refresh the last confirmed observation for groups that remain owned.
fn heartbeat() {
    loop {
        std::thread::sleep(HEARTBEAT);
        if let Some(guard) = guard().as_mut()
            && let Err(error) = persist(guard)
        {
            crate::diagnostic(format_args!(
                "could not refresh owned process groups: {error}"
            ));
        }
    }
}

fn persist(guard: &mut Guard) -> io::Result<()> {
    let observed_at = now();
    let entries = guard
        .groups
        .iter()
        .map(|group| {
            Record::ProcessGroup(GroupRecord {
                id: group.id,
                source_id: None,
                pgid: group.pgid,
                spawned_at: group.spawned_at,
                observed_at,
                origin: group.origin.clone(),
                evidence: group.evidence.clone(),
            })
        })
        .chain(guard.retained.iter().cloned())
        .chain(
            guard
                .intents
                .iter()
                .map(|intent| Record::LaunchIntent(intent.entry.clone())),
        )
        .collect::<Vec<_>>();
    // Retained groups are unproven, so only the owned set reaches the watchdog.
    let document = Ledger::new(entries);
    let result = document.to_disk_bytes().and_then(|bytes| {
        write_watch_list_at(&guard.watch_list, &guard.groups, observed_at)
            .and_then(|()| write_ledger_atomically(&guard.ledger, &bytes))
    });
    if result.is_err() {
        guard.sources_clean = false;
    }
    result
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

fn remove_watch_paths(paths: &[PathBuf]) -> io::Result<()> {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_watch_list(path: &Path, groups: &[OwnedGroup]) -> io::Result<()> {
    write_watch_list_at(path, groups, now())
}

fn write_watch_list_at(path: &Path, groups: &[OwnedGroup], observed_at: u64) -> io::Result<()> {
    let mut line = observed_at.to_string();
    #[cfg(target_os = "linux")]
    let mut versioned = false;
    for group in groups {
        #[cfg(target_os = "linux")]
        if let GroupEvidence::Captured(identity) = &group.evidence
            && group.origin.as_ref() == Some(&identity.origin)
            && identity.origin.valid()
            && group.pgid > 1
            && group.spawned_at != 0
        {
            let time_namespace = identity
                .origin
                .time_namespace
                .map_or_else(|| "-".to_owned(), |inode| inode.to_string());
            let proof = match identity.strength {
                IdentityStrength::Pidfs { inode } if inode != 0 => format!("p{inode}"),
                IdentityStrength::Approximate { reason } => match reason {
                    UnavailableReason::PidfdUnavailable => "a1".to_owned(),
                    UnavailableReason::PidfdDenied => "a2".to_owned(),
                    UnavailableReason::LegacyPidfd => "a3".to_owned(),
                },
                _ => continue,
            };
            if !versioned {
                line.push_str(" v2");
                versioned = true;
            }
            line.push_str(&format!(
                " {}:{}:{}:{}:{}:{}:{}:{}",
                group.pgid,
                group.spawned_at,
                identity.start_ticks,
                identity.origin.boot_id,
                identity.origin.pid_namespace,
                time_namespace,
                group.id,
                proof
            ));
        }
        #[cfg(not(target_os = "linux"))]
        {
            line.push(' ');
            line.push_str(&format!("{}:{}", group.pgid, group.spawned_at));
        }
    }
    line.push('\n');
    write_atomically(path, line.as_bytes())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".tmp.{}", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.flush()?;
    drop(file);
    std::fs::rename(temporary, path)
}

/// Never overwrite an earlier interrupted ledger publication: it may be the
/// only evidence that a protected child was launched. The new temporary name
/// is itself an input to the next recovery if this write is interrupted.
fn write_ledger_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically(path, bytes)
}

fn watchdog_command(watch_list: &Path) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", WATCHDOG, "cccc-watchdog"])
        .arg(watch_list)
        .arg(WATCH_LIST_MAX_AGE.as_secs().to_string())
        .arg(TIME_TOLERANCE_SECS.to_string());
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

/// Read either the original list or the versioned evidence envelope. A legacy
/// item's ID binds its source name, position and fields, so replay after an
/// interrupted merge recognizes the same source without merging distinct ones.
/// Persist the result before removing any source watch list.
fn read_ledger_records(ledger: &Path) -> io::Result<Vec<Record>> {
    let file = match File::open(ledger) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let bytes = read_ledger_bytes(file)?;
    let name = ledger
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid guard ledger name"))?;
    parse_ledger_records(&bytes, name.as_bytes())
}

fn parse_ledger_records(bytes: &[u8], source: &[u8]) -> io::Result<Vec<Record>> {
    if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'[')
    {
        let old: Vec<LedgerEntry> = serde_json::from_slice(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        return legacy_records(old, source);
    }
    Ok(Ledger::from_bytes(bytes)?.records)
}

fn read_ledger_bytes(file: File) -> io::Result<Vec<u8>> {
    const MAX_LEDGER_BYTES: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(MAX_LEDGER_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "guard publication exceeds size limit",
        ));
    }
    Ok(bytes)
}

/// Read the committed ledger plus every interrupted publication. Returning an
/// error preserves the files; a partial file is never an empty owned set.
fn read_ledger_sources(ledger: &Path) -> io::Result<(Vec<Record>, Vec<PathBuf>)> {
    let mut records = read_ledger_records(ledger)?;
    let Some(directory) = ledger.parent() else {
        return Ok((records, Vec::new()));
    };
    let Some(name) = ledger.file_name().and_then(|name| name.to_str()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid guard ledger name",
        ));
    };
    let prefix = format!("{name}.tmp");
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((records, Vec::new()));
        }
        Err(error) => return Err(error),
    };
    let mut files = entries.collect::<io::Result<Vec<_>>>()?;
    files.sort_by_key(std::fs::DirEntry::file_name);
    let mut sources = Vec::new();
    for entry in files {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name != prefix && !name.starts_with(&format!("{prefix}.")) {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "interrupted guard publication is not a regular file",
            ));
        }
        let path = entry.path();
        let bytes = read_ledger_bytes(File::open(&path)?)?;
        records.extend(parse_ledger_records(&bytes, entry.file_name().as_bytes())?);
        sources.push(path);
    }
    Ok((records, sources))
}

fn legacy_records(entries: Vec<LedgerEntry>, source: &[u8]) -> io::Result<Vec<Record>> {
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if entry.pgid <= 1 || entry.spawned_at == 0 || entry.observed_at == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid legacy guard record",
                ));
            }
            let mut digest = Sha256::new();
            digest.update(b"cccc-legacy-process-evidence-v1\0");
            digest.update((source.len() as u64).to_le_bytes());
            digest.update(source);
            digest.update((index as u64).to_le_bytes());
            digest.update(entry.pgid.to_le_bytes());
            digest.update(entry.spawned_at.to_le_bytes());
            digest.update(entry.observed_at.to_le_bytes());
            let hash = digest.finalize();
            let mut id = [0; 16];
            id.copy_from_slice(&hash[..16]);
            // RFC 9562 UUIDv8 carries this application-specific digest; the
            // fixed version/variant bits also exclude an all-zero identifier.
            id[6] = (id[6] & 0x0f) | 0x80;
            id[8] = (id[8] & 0x3f) | 0x80;
            Ok(Record::ProcessGroup(GroupRecord {
                id: uuid::Uuid::from_bytes(id),
                source_id: None,
                pgid: entry.pgid,
                spawned_at: entry.spawned_at,
                observed_at: entry.observed_at,
                origin: None,
                evidence: GroupEvidence::Legacy,
            }))
        })
        .collect()
}

/// Multiple interrupted snapshots can contain the same ID. Identical copies
/// are one item; conflicting versions must both remain visible rather than
/// selecting one without evidence of ordering.
fn normalize_source_records(records: &mut Vec<Record>) -> io::Result<()> {
    let mut normalized: Vec<Record> = Vec::with_capacity(records.len());
    let mut seen_records = std::collections::HashSet::new();
    let mut seen_ids = std::collections::HashSet::new();
    for mut record in records.drain(..) {
        let original_record = record.original();
        if !seen_records.insert(original_record.clone()) {
            continue;
        }
        let original = serde_json::to_vec(&original_record)?;
        let mut collision = 0_u64;
        while !seen_ids.insert(record.id()) {
            let mut digest = Sha256::new();
            digest.update(b"cccc-conflicting-process-evidence-v1\0");
            digest.update(&original);
            digest.update(collision.to_le_bytes());
            let hash = digest.finalize();
            let mut id = [0; 16];
            id.copy_from_slice(&hash[..16]);
            id[6] = (id[6] & 0x0f) | 0x80;
            id[8] = (id[8] & 0x3f) | 0x80;
            let replacement = uuid::Uuid::from_bytes(id);
            if replacement == original_record.id() {
                collision += 1;
                continue;
            }
            record.set_id(replacement);
            // Keep the source identity even if the other conflicting record
            // is later cleared before an interrupted source is removed.
            record.set_source_id(Some(original_record.id()));
            collision += 1;
        }
        normalized.push(record);
    }
    *records = normalized;
    Ok(())
}

/// Reconcile using the origin and identity actually recorded for each group.
/// A legacy group may only be forgotten when absent in the confirmed local PID
/// view; an incomplete launch never acquires a group identity by inference.
#[cfg(target_os = "linux")]
fn reconcile_records(records: Vec<Record>) -> Vec<Record> {
    use rustix::process::Signal;
    let Ok(view) = super::linux_identity::ProcView::open() else {
        return records;
    };
    let Ok(current_origin) = view.origin() else {
        return records;
    };
    let mut retained = Vec::with_capacity(records.len());
    let mut term_sent = Vec::with_capacity(records.len());
    for record in records {
        let record_origin = match &record {
            Record::LaunchIntent(intent) => intent.origin.as_ref(),
            Record::ProcessGroup(group) => group.origin.as_ref(),
        };
        if record_origin.is_some_and(|previous| previous.boot_id != current_origin.boot_id) {
            // Under the accepted one-host/home premise a verified boot change
            // ends local work from that boot, without sending a signal.
            continue;
        }
        let mut sent = false;
        if let Record::ProcessGroup(group) = &record {
            if group.evidence == GroupEvidence::Legacy && group_absent(group.pgid) {
                continue;
            }
            if matches!(group.evidence, GroupEvidence::Captured(_))
                && group.origin.as_ref() == Some(&current_origin)
            {
                if leader_and_group_absent(group.pgid) {
                    continue;
                }
                sent = view.signal_group(group, Signal::TERM).is_ok();
            }
        }
        retained.push(record);
        term_sent.push(sent);
    }

    if term_sent.iter().any(|sent| *sent) {
        wait_for_groups(&retained, &term_sent, TERMINATE_GRACE);
        for (record, sent) in retained.iter().zip(&term_sent) {
            if *sent
                && let Record::ProcessGroup(group) = record
                && !group_absent(group.pgid)
            {
                // Recheck the original evidence and every target before KILL.
                let _ = view.signal_group(group, Signal::KILL);
            }
        }
        wait_for_groups(&retained, &term_sent, Duration::from_millis(250));
    }
    retained
        .into_iter()
        .filter(|record| match record {
            Record::ProcessGroup(group) if group.evidence == GroupEvidence::Legacy => {
                !group_absent(group.pgid)
            }
            Record::ProcessGroup(group)
                if matches!(group.evidence, GroupEvidence::Captured(_))
                    && group.origin.as_ref() == Some(&current_origin) =>
            {
                !leader_and_group_absent(group.pgid)
            }
            _ => true,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn leader_and_group_absent(pgid: i32) -> bool {
    // A leader can leave its original process group and later recreate it.
    // An empty PGID alone therefore does not establish that the leader ended.
    // Use signal 0 rather than a missing proc row: hidepid can make a live,
    // inaccessible process look absent in procfs. EPERM is not absence.
    rustix::process::Pid::from_raw(pgid).is_some_and(|pid| {
        matches!(
            rustix::process::test_kill_process(pid),
            Err(rustix::io::Errno::SRCH)
        ) && group_absent(pgid)
    })
}

#[cfg(not(target_os = "linux"))]
fn reconcile_records(records: Vec<Record>) -> Vec<Record> {
    records
        .into_iter()
        .filter(|record| {
            !matches!(
                record,
                Record::ProcessGroup(group)
                    if group.evidence == GroupEvidence::Legacy && group_absent(group.pgid)
            )
        })
        .collect()
}

fn group_absent(pgid: i32) -> bool {
    rustix::process::Pid::from_raw(pgid).is_some_and(|pid| {
        matches!(
            rustix::process::test_kill_process_group(pid),
            Err(rustix::io::Errno::SRCH)
        )
    })
}

#[cfg(target_os = "linux")]
fn wait_for_groups(records: &[Record], selected: &[bool], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while records.iter().zip(selected).any(|(record, selected)| {
        *selected && matches!(record, Record::ProcessGroup(group) if !group_absent(group.pgid))
    }) && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;

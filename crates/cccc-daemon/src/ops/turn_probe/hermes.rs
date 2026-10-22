use super::TurnObservation;
use super::process::{Process, Processes, owned};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub(super) struct Hermes {
    // Learned only from a live owned lease, never from session id/cwd similarity.
    sessions: BTreeMap<String, Process>,
    database_identity: Option<(u64, u64)>,
}

impl Hermes {
    pub fn observe(
        &mut self,
        path: &Path,
        processes: &impl Processes,
        root: &Process,
        now: f64,
    ) -> TurnObservation {
        match self.read(path, processes, root, now) {
            Ok(value) => value,
            Err(why) => {
                if matches!(
                    why,
                    "holder_generation_mismatch"
                        | "session_binding_changed"
                        | "holder_left_tree"
                        | "generation_changed"
                        | "process_unavailable"
                        | "process_not_live"
                        | "database_replaced"
                ) {
                    self.sessions.clear();
                }
                TurnObservation::Unknown { why }
            }
        }
    }

    fn read(
        &mut self,
        path: &Path,
        processes: &impl Processes,
        root: &Process,
        now: f64,
    ) -> Result<TurnObservation, &'static str> {
        if !now.is_finite() {
            return Err("clock_invalid");
        }
        let identity = database_identity(path)?;
        if self.database_identity.is_some_and(|old| old != identity) {
            self.database_identity = Some(identity);
            return Err("database_replaced");
        }
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| "database_unavailable")?;
        db.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 8192);
        db.busy_timeout(Duration::from_millis(25))
            .map_err(|_| "database_unavailable")?;
        let deadline = Instant::now() + Duration::from_millis(50);
        db.progress_handler(1000, Some(move || Instant::now() >= deadline));
        db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; BEGIN")
            .map_err(|_| "database_unavailable")?;
        let versions = db
            .prepare("SELECT version FROM schema_version LIMIT 2")
            .and_then(|mut q| {
                q.query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|_| "schema_unavailable")?;
        if versions != [31] {
            return Err("schema_unsupported");
        }
        let mut query = db.prepare(
            "SELECT conversation_id, holder, acquired_at, expires_at FROM session_turn_leases LIMIT 257")
            .map_err(|_| "schema_unavailable")?;
        let leases = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|_| "database_unavailable")?;
        if leases.len() > 256 {
            return Err("lease_limit");
        }
        let mut working_since: Option<f64> = None;
        let mut learned = self.sessions.clone();
        for (session, holder, acquired, expiry) in leases {
            if Instant::now() >= deadline {
                return Err("probe_budget");
            }
            if !acquired.is_finite() || !expiry.is_finite() || expiry < acquired {
                return Err("lease_invalid");
            }
            if expiry <= now {
                continue;
            }
            let (pid, namespace, cli) = parse_holder(&holder).ok_or("holder_unsupported")?;
            // A foreign namespace cannot establish membership in this process tree.
            if namespace != root.namespace {
                return Err("namespace_mismatch");
            }
            let Some(process) = owned(processes, root, pid)? else {
                continue;
            };
            if !cli {
                return Err("platform_unsupported");
            }
            if acquired < process.born_before || acquired > now {
                return Err("holder_generation_mismatch");
            }
            if learned
                .get(&session)
                .is_some_and(|old| old.pid != process.pid || old.start != process.start)
            {
                return Err("session_binding_changed");
            }
            learned.insert(session, process);
            working_since = Some(working_since.map_or(acquired, |old| old.min(acquired)));
        }
        if learned.len() > 256 {
            return Err("session_limit");
        }
        // Recheck every provider identity. A dead/reused holder is never proof of idle.
        let mut live_sessions = 0;
        for (session, bound) in &learned {
            if Instant::now() >= deadline {
                return Err("probe_budget");
            }
            let current = owned(processes, root, bound.pid)?.ok_or("holder_left_tree")?;
            if current.start != bound.start || current.namespace != bound.namespace {
                return Err("holder_generation_mismatch");
            }
            let ended = db
                .query_row(
                    "SELECT ended_at FROM sessions WHERE id=?1",
                    [session],
                    |row| row.get::<_, Option<f64>>(0),
                )
                .optional()
                .map_err(|_| "session_unavailable")?;
            if ended == Some(None) {
                live_sessions += 1;
            }
        }
        let current_root = processes.read(root.pid)?;
        if current_root.start != root.start || current_root.namespace != root.namespace {
            return Err("generation_changed");
        }
        db.execute_batch("ROLLBACK")
            .map_err(|_| "database_unavailable")?;
        if database_identity(path)? != identity {
            return Err("database_replaced");
        }
        if Instant::now() >= deadline {
            return Err("probe_budget");
        }
        self.sessions = learned;
        self.database_identity = Some(identity);
        if let Some(since) = working_since {
            Ok(TurnObservation::Working {
                since: timestamp(since),
            })
        } else if live_sessions > 0 {
            Ok(TurnObservation::Idle {
                since: timestamp(now),
            })
        } else {
            Err("session_not_bound")
        }
    }
}

#[cfg(target_os = "linux")]
fn database_identity(path: &Path) -> Result<(u64, u64), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).map_err(|_| "database_unavailable")?;
    if !metadata.is_file() {
        return Err("database_not_regular");
    }
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(target_os = "linux"))]
fn database_identity(_path: &Path) -> Result<(u64, u64), &'static str> {
    Err("database_identity_unsupported")
}

fn timestamp(seconds: f64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis((seconds * 1000.0) as i64)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn parse_holder(holder: &str) -> Option<(u32, u64, bool)> {
    let (pid, rest) = holder.strip_prefix("pid=")?.split_once(":pidns=")?;
    let (namespace, turn) = rest.split_once(":turn=")?;
    let (turn, platform) = turn.rsplit_once(":platform=")?;
    if turn.is_empty() || pid.is_empty() || namespace.is_empty() {
        return None;
    }
    Some((
        pid.parse().ok()?,
        namespace.parse().ok()?,
        platform == "cli",
    ))
}

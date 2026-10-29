//! Bounded, content-free diagnostics for the CLI-owned daemon.
//!
//! Tracing fields may contain provider output or other private data. This
//! subscriber records only source locations and explicitly allowed identifiers
//! and codes. It deliberately never formats a `message` or `error` field.

use fs2::FileExt;
use serde_json::json;
use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

pub(crate) const NAME: &str = "ccccd-diagnostics.log";
pub(crate) const PREVIOUS: &str = "ccccd-diagnostics.log.1";
const LOCK: &str = "ccccd-diagnostics.lock";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_KEYS: usize = 256;
const REPEAT_INTERVAL: Duration = Duration::from_secs(60);

pub(crate) fn install(dir: &Path) {
    let state = Arc::new(Mutex::new(State::new(dir.to_path_buf(), MAX_BYTES)));
    let subscriber = tracing_subscriber::registry().with(SafeLayer(Arc::clone(&state)));
    // An embedding application's subscriber remains authoritative.
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        return;
    }
    let initialized = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .marker("start");
    if initialized.is_err() {
        cccc_runtime::diagnostic(format_args!("CCCC could not initialize daemon diagnostics"));
    }
}

struct SafeLayer(Arc<Mutex<State>>);

impl<S> Layer<S> for SafeLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        *metadata.level() <= Level::WARN
            && matches!(
                metadata.target().split("::").next(),
                Some("cccc_daemon" | "cccc_core" | "cccc_runtime")
            )
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = SafeFields::default();
        event.record(&mut fields);
        let metadata = event.metadata();
        let event_id = fields.event_id;
        let key = Key {
            level: metadata.level().as_str().to_owned(),
            module: metadata.target().to_owned(),
            file: metadata.file().and_then(safe_source_file),
            line: metadata.line().unwrap_or_default(),
            group_id: fields.group_id,
            actor_id: fields.actor_id,
            code: fields.code,
            action: fields.action,
        };
        // Another diagnostic may be doing slow file I/O while it holds the
        // state lock. Losing this best-effort record must not make an
        // unrelated daemon operation wait for that write.
        let mut state = match self.0.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        let previous_rate = state.rates.get(&key).cloned();
        let Some(suppressed) = state.allow(&key, Instant::now()) else {
            return;
        };
        let mut record = json!({
            "kind":"event", "ts":cccc_contracts::utc_now(),
            "version":env!("CARGO_PKG_VERSION"), "source_id":state.source_id,
            "level":key.level, "module":key.module, "line":key.line,
        });
        for (name, value) in [
            ("file", key.file.as_deref()),
            ("group_id", key.group_id.as_deref()),
            ("actor_id", key.actor_id.as_deref()),
            ("event_id", event_id.as_deref()),
            ("code", key.code.as_deref()),
            ("action", key.action.as_deref()),
        ] {
            if let Some(value) = value {
                record[name] = json!(value);
            }
        }
        if suppressed > 0 {
            record["suppressed"] = json!(suppressed);
        }
        // Diagnostics are best effort: failure cannot affect a daemon action.
        if let Err(error) = state.write(&record) {
            if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                // A concurrent reader/clear must not cause the next repeated
                // warning to be throttled when this one was never written.
                if let Some(previous_rate) = previous_rate {
                    state.rates.insert(key, previous_rate);
                } else {
                    state.rates.remove(&key);
                }
            } else if !state.reported_failure {
                state.reported_failure = true;
                cccc_runtime::diagnostic(format_args!("CCCC could not write daemon diagnostics"));
            }
        }
    }
}

#[derive(Default)]
struct SafeFields {
    group_id: Option<String>,
    actor_id: Option<String>,
    event_id: Option<String>,
    code: Option<String>,
    action: Option<String>,
}

impl SafeFields {
    fn record(&mut self, name: &str, value: &str) {
        let value = value.trim_matches('"');
        let is_id = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'));
        match name {
            "group_id" if is_id => self.group_id = Some(value.into()),
            "actor_id" if is_id => self.actor_id = Some(value.into()),
            "event_id" if is_id => self.event_id = Some(value.into()),
            "diagnostic_code" if safe_code(value) => self.code = Some(value.into()),
            "diagnostic_action" if safe_action(value) => self.action = Some(value.into()),
            _ => {}
        }
    }
}

impl Visit for SafeFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field.name(), value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if !matches!(
            field.name(),
            "group_id" | "actor_id" | "event_id" | "diagnostic_code" | "diagnostic_action"
        ) {
            return;
        }
        let mut out = LimitedString::default();
        if write!(out, "{value:?}").is_ok() {
            self.record(field.name(), &out.0);
        }
    }
}

#[derive(Default)]
struct LimitedString(String);

impl fmt::Write for LimitedString {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.0.len().saturating_add(value.len()) > 66 {
            return Err(fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}

fn safe_code(value: &str) -> bool {
    matches!(
        value,
        "not_found"
            | "permission_denied"
            | "invalid_data"
            | "timed_out"
            | "already_exists"
            | "broken_pipe"
            | "connection_refused"
            | "connection_reset"
            | "interrupted"
            | "io_error"
            | "runtime_error"
            | "other"
    )
}

fn safe_action(value: &str) -> bool {
    matches!(
        value,
        "runtime_reap"
            | "runtime_reconcile"
            | "automation_group_discovery"
            | "automation_group_tick"
            | "actor_activity"
            | "ledger_decode"
    )
}

fn safe_source_file(value: &str) -> Option<String> {
    let normalized = value.replace('\\', "/");
    let start = normalized.rfind("crates/cccc-")?;
    let relative = &normalized[start..];
    let valid = relative.len() <= 192
        && !relative
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        && relative
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b'/' | b'.'));
    valid.then(|| relative.to_owned())
}

pub(crate) fn io_code(error: &io::Error) -> &'static str {
    if error
        .get_ref()
        .is_some_and(|source| source.is::<serde_json::Error>() || source.is::<serde_yaml::Error>())
    {
        return "invalid_data";
    }
    match error.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::TimedOut => "timed_out",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::BrokenPipe => "broken_pipe",
        io::ErrorKind::ConnectionRefused => "connection_refused",
        io::ErrorKind::ConnectionReset => "connection_reset",
        io::ErrorKind::Interrupted => "interrupted",
        _ => "other",
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    level: String,
    module: String,
    file: Option<String>,
    line: u32,
    group_id: Option<String>,
    actor_id: Option<String>,
    code: Option<String>,
    action: Option<String>,
}

#[derive(Clone)]
struct Rate {
    last: Instant,
    suppressed: u64,
}

struct State {
    dir: PathBuf,
    source_id: String,
    limit: u64,
    rates: HashMap<Key, Rate>,
    reported_failure: bool,
}

impl State {
    fn new(dir: PathBuf, limit: u64) -> Self {
        let source_id = cccc_core::build_info::current()["source_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        Self {
            dir,
            source_id,
            limit,
            rates: HashMap::new(),
            reported_failure: false,
        }
    }

    fn allow(&mut self, key: &Key, now: Instant) -> Option<u64> {
        if let Some(rate) = self.rates.get_mut(key) {
            if now.saturating_duration_since(rate.last) < REPEAT_INTERVAL {
                rate.suppressed = rate.suppressed.saturating_add(1);
                return None;
            }
            rate.last = now;
            return Some(std::mem::take(&mut rate.suppressed));
        }
        if self.rates.len() >= MAX_KEYS
            && let Some(oldest) = self
                .rates
                .iter()
                .min_by_key(|(_, value)| value.last)
                .map(|(key, _)| key.clone())
        {
            self.rates.remove(&oldest);
        }
        self.rates.insert(
            key.clone(),
            Rate {
                last: now,
                suppressed: 0,
            },
        );
        Some(0)
    }

    fn marker(&mut self, kind: &str) -> io::Result<()> {
        self.write(&json!({"kind":kind,"ts":cccc_contracts::utc_now(),"version":env!("CARGO_PKG_VERSION"),"source_id":self.source_id}))
    }

    fn write(&mut self, record: &serde_json::Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(record)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > self.limit {
            return Err(io::Error::other("diagnostic record exceeds file limit"));
        }
        let dir = self.dir.clone();
        with_lock(&dir, true, || {
            let path = dir.join(NAME);
            let size = match fs::metadata(&path) {
                Ok(meta) => meta.len(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            };
            if size.saturating_add(bytes.len() as u64) > self.limit {
                let previous = dir.join(PREVIOUS);
                match fs::remove_file(&previous) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                fs::rename(&path, previous)?;
                let mut marker = serde_json::to_vec(
                    &json!({"kind":"rotation","ts":cccc_contracts::utc_now(),"version":env!("CARGO_PKG_VERSION"),"source_id":self.source_id}),
                )?;
                marker.push(b'\n');
                if (marker.len() + bytes.len()) as u64 <= self.limit {
                    open_append(&path)?.write_all(&marker)?;
                }
            }
            open_append(&path)?.write_all(&bytes)
        })
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(crate) fn with_lock<T>(
    dir: &Path,
    nonblocking: bool,
    run: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(dir.join(LOCK))?;
    if nonblocking {
        lock.try_lock_exclusive()?;
    } else {
        lock.lock_exclusive()?;
    }
    let result = run();
    let unlock = FileExt::unlock(&lock);
    result.and_then(|value| unlock.map(|()| value))
}

pub(crate) fn clear(dir: &Path) -> io::Result<()> {
    with_lock(dir, false, || {
        for name in [NAME, PREVIOUS] {
            match OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(dir.join(name))
            {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        Key {
            level: "WARN".into(),
            module: "cccc_daemon::fixture".into(),
            file: Some("crates/cccc-daemon/src/fixture.rs".into()),
            line: 42,
            group_id: Some("g_fixture".into()),
            actor_id: Some("agent-1".into()),
            code: None,
            action: None,
        }
    }

    #[test]
    fn only_explicit_safe_fields_are_recorded() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let state = Arc::new(Mutex::new(State::new(temp.path().into(), MAX_BYTES)));
        let subscriber = tracing_subscriber::registry().with(SafeLayer(state));
        tracing::subscriber::with_default(subscriber, || {
            let private = "private_fixture_token_sk_forbidden";
            let group_id = "g_fixture";
            let actor_id = "agent-1";
            let code = "invalid_data";
            tracing::warn!(target: "cccc_daemon::fixture", %group_id, %actor_id, diagnostic_code = %code, diagnostic_action = "automation_group_tick", error = %private, message = %private, "{private}");
            tracing::info!(target: "cccc_daemon::fixture", "must not be recorded");
            tracing::error!(target: "external::fixture", "must not be recorded");
        });
        let text = fs::read_to_string(temp.path().join(NAME)).expect("diagnostics");
        let records: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON line"))
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["group_id"], "g_fixture");
        assert_eq!(records[0]["actor_id"], "agent-1");
        assert_eq!(records[0]["code"], "invalid_data");
        assert_eq!(records[0]["action"], "automation_group_tick");
        assert!(records[0]["line"].as_u64().is_some());
        assert_eq!(
            records[0]["file"],
            "crates/cccc-daemon/src/diagnostic_log.rs"
        );
        assert!(!text.contains("private_fixture"));
        assert!(!text.contains("must not be recorded"));
    }

    #[test]
    fn a_busy_writer_does_not_block_another_diagnostic() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let state = Arc::new(Mutex::new(State::new(temp.path().into(), MAX_BYTES)));
        let guard = state.lock().expect("hold diagnostic state");
        let subscriber = tracing_subscriber::registry().with(SafeLayer(Arc::clone(&state)));
        let (sent, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tracing::subscriber::with_default(subscriber, || {
                tracing::warn!(target: "cccc_daemon::fixture", "busy writer");
                sent.send(()).expect("report completion");
            });
        });
        let completed_while_busy = received.recv_timeout(Duration::from_secs(2));
        drop(guard);
        worker.join().expect("diagnostic worker");
        assert!(
            completed_while_busy.is_ok(),
            "diagnostic waited for the busy writer"
        );
        assert!(
            !temp.path().join(NAME).exists(),
            "busy record was not skipped"
        );
    }

    #[test]
    fn unrecognized_codes_and_actions_are_not_logged() {
        let mut fields = SafeFields::default();
        fields.record("diagnostic_code", "private-code");
        fields.record("diagnostic_action", "private_action");
        fields.record("group_id", "not a valid id");
        assert!(fields.code.is_none() && fields.action.is_none() && fields.group_id.is_none());
        assert_eq!(
            safe_source_file("/private/build/crates/cccc-daemon/src/server.rs"),
            Some("crates/cccc-daemon/src/server.rs".into())
        );
        assert!(safe_source_file("/private/project/src/server.rs").is_none());
        assert!(safe_source_file("crates/cccc-daemon/../../secret").is_none());
    }

    #[test]
    fn typed_parse_errors_keep_a_safe_category() {
        let parse = serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON");
        assert_eq!(io_code(&io::Error::other(parse)), "invalid_data");
        assert_eq!(
            io_code(&io::Error::from(io::ErrorKind::PermissionDenied)),
            "permission_denied"
        );
        assert_eq!(io_code(&io::Error::other("private error text")), "other");
    }

    #[test]
    fn runtime_reap_error_has_a_safe_category() {
        let mut fields = SafeFields::default();
        fields.record("diagnostic_code", "runtime_error");
        fields.record("diagnostic_action", "runtime_reap");
        assert_eq!(fields.code.as_deref(), Some("runtime_error"));
        assert_eq!(fields.action.as_deref(), Some("runtime_reap"));
    }

    #[test]
    fn invalid_ledger_records_identify_the_group_without_content_or_paths() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let group = temp.path().join("g_fixture");
        fs::create_dir(&group).expect("fixture group");
        let ledger = group.join("ledger.jsonl");
        fs::write(&ledger, "{private_fixture_token_must_not_be_logged}\n").expect("invalid ledger");
        let state = Arc::new(Mutex::new(State::new(temp.path().into(), MAX_BYTES)));
        let subscriber = tracing_subscriber::registry().with(SafeLayer(state));
        tracing::subscriber::with_default(subscriber, || {
            assert!(
                cccc_core::ledger::read_all(&ledger)
                    .expect("ledger read")
                    .is_empty()
            );
        });
        let text = fs::read_to_string(temp.path().join(NAME)).expect("diagnostics");
        let record: serde_json::Value = serde_json::from_str(text.trim()).expect("JSON line");
        assert_eq!(record["group_id"], "g_fixture");
        assert_eq!(record["code"], "invalid_data");
        assert_eq!(record["action"], "ledger_decode");
        assert!(!text.contains("private_fixture"));
        let fixture_root = temp.path().to_string_lossy();
        assert!(!text.contains(fixture_root.as_ref()));
    }

    #[test]
    fn repeated_events_are_limited_and_report_the_suppressed_count() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let mut state = State::new(temp.path().into(), MAX_BYTES);
        let now = Instant::now();
        let key = key();
        assert_eq!(state.allow(&key, now), Some(0));
        assert_eq!(state.allow(&key, now), None);
        assert_eq!(state.allow(&key, now), None);
        assert_eq!(state.allow(&key, now + REPEAT_INTERVAL), Some(2));
        for line in 0..(MAX_KEYS as u32 + 3) {
            let mut another = key.clone();
            another.line = line;
            state.allow(&another, now);
        }
        assert!(state.rates.len() <= MAX_KEYS);
    }

    #[test]
    fn different_event_ids_at_the_same_location_are_rate_limited_together() {
        fn emit(event_id: &str) {
            tracing::warn!(target: "cccc_daemon::fixture", group_id = "g_fixture", %event_id, "fixture");
        }
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let state = Arc::new(Mutex::new(State::new(temp.path().into(), MAX_BYTES)));
        let subscriber = tracing_subscriber::registry().with(SafeLayer(Arc::clone(&state)));
        tracing::subscriber::with_default(subscriber, || {
            emit("event_1");
            emit("event_2");
            emit("event_3");
        });
        let text = fs::read_to_string(temp.path().join(NAME)).expect("diagnostics");
        let records: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON line"))
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["event_id"], "event_1");
        let state = state.lock().expect("state");
        assert_eq!(state.rates.values().next().expect("rate").suppressed, 2);
    }

    #[test]
    fn lock_contention_does_not_throttle_a_later_record() {
        fn emit() {
            tracing::warn!(target: "cccc_daemon::fixture", group_id = "g_fixture", "fixture");
        }
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let state = Arc::new(Mutex::new(State::new(temp.path().into(), MAX_BYTES)));
        let subscriber = tracing_subscriber::registry().with(SafeLayer(state));
        tracing::subscriber::with_default(subscriber, || {
            with_lock(temp.path(), false, || {
                emit();
                Ok(())
            })
            .expect("hold diagnostic lock");
            emit();
        });
        let contents = fs::read_to_string(temp.path().join(NAME)).expect("later warning");
        assert_eq!(contents.lines().count(), 1);
    }

    #[test]
    fn rotation_bounds_both_files_and_records_the_build() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let mut state = State::new(temp.path().into(), 512);
        state.marker("start").expect("startup marker");
        for _ in 0..12 {
            state
                .write(&json!({"kind":"event","padding":"x".repeat(100)}))
                .expect("event");
        }
        for name in [NAME, PREVIOUS] {
            let file = temp.path().join(name);
            assert!(fs::metadata(&file).expect("rotated file").len() <= 512);
            let text = fs::read_to_string(file).expect("rotated contents");
            assert!(text.contains("rotation"));
            assert!(text.contains(&state.source_id));
        }
    }

    #[test]
    fn failure_to_write_diagnostics_does_not_panic() {
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let state = Arc::new(Mutex::new(State::new(
            temp.path().join("missing"),
            MAX_BYTES,
        )));
        let subscriber = tracing_subscriber::registry().with(SafeLayer(state));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "cccc_daemon::fixture", "this is safe to drop");
        });
        assert!(!temp.path().join("missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn log_and_lock_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temporary diagnostics");
        let mut state = State::new(temp.path().into(), MAX_BYTES);
        state.marker("start").expect("startup marker");
        for name in [NAME, LOCK] {
            assert_eq!(
                fs::metadata(temp.path().join(name))
                    .expect("file")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

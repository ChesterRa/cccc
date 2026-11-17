//! A supervised update's startup fence. The installer owns its replacement
//! phases; this record only reserves one home's next daemon start.

use crate::HomeLayout;
use crate::local_sessions::NativeHost;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const SCHEMA: u32 = 1;
const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildIdentity {
    pub version: String,
    pub source_id: String,
}

impl BuildIdentity {
    /// A library embedding without a registered executable identity can start
    /// normally, but cannot claim to be a particular update candidate.
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            source_id: crate::build_info::source_id().unwrap_or_default().into(),
        }
    }

    fn validate(&self) -> io::Result<()> {
        if self.version.is_empty()
            || self.version.len() > 128
            || !self
                .version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
            || self.source_id.len() != 64
            || !self.source_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("invalid update target build identity"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    schema: u32,
    pub transaction_id: String,
    pub transaction_dir: PathBuf,
    pub target: BuildIdentity,
    pub install_dir: PathBuf,
    pub holder: NativeHost,
}

impl Intent {
    /// The caller has already prepared this private transaction directory and
    /// checked the candidate. Paths are bound to this home before publication.
    pub fn new(
        home: &HomeLayout,
        transaction_id: &str,
        target: BuildIdentity,
        install_dir: PathBuf,
    ) -> io::Result<Self> {
        let intent = Self {
            schema: SCHEMA,
            transaction_id: transaction_id.into(),
            transaction_dir: transaction_dir(home, transaction_id)?,
            target,
            install_dir: fs::canonicalize(install_dir)?,
            holder: NativeHost::capture(std::process::id())?,
        };
        intent.validate(home)?;
        Ok(intent)
    }

    /// Before apply, the installed executable is still the old version. After
    /// apply, prefer the installer's retained old executable for recovery.
    pub fn recovery_command(&self) -> String {
        let executable = if cfg!(windows) { "cccc.exe" } else { "cccc" };
        let backup = self
            .install_dir
            .join(format!(".cccc-backup.{}", self.transaction_id))
            .join(executable);
        let program = if backup.is_file() {
            backup
        } else {
            self.install_dir.join(executable)
        };
        #[cfg(windows)]
        let quote = |path: &Path| crate::local_sessions::powershell_quote(&path.to_string_lossy());
        #[cfg(not(windows))]
        let quote = |path: &Path| shell_words::quote(&path.to_string_lossy()).into_owned();
        let command = format!(
            "{} update --recover {}",
            quote(&program),
            quote(&self.transaction_dir)
        );
        if cfg!(windows) {
            format!("& {command}")
        } else {
            command
        }
    }

    fn validate(&self, home: &HomeLayout) -> io::Result<()> {
        if self.schema != SCHEMA
            || self.transaction_dir != transaction_dir(home, &self.transaction_id)?
            || !self.install_dir.is_absolute()
            || !self.install_dir.is_dir()
            || fs::canonicalize(&self.install_dir)? != self.install_dir
            || self.holder.pid == 0
            || self.holder.started.is_empty()
            || self.holder.boot.is_empty()
        {
            return Err(invalid("invalid update intent"));
        }
        self.target.validate()
    }

    fn require_executor(&self) -> io::Result<()> {
        if NativeHost::capture(std::process::id())? == self.holder || self.holder.has_exited() {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "update_in_progress: update host PID {} still owns transaction {} (or its exit cannot be confirmed)",
                self.holder.pid, self.transaction_id
            ),
        ))
    }
}

/// Reads do not create an update directory, lock, daemon or other runtime work.
/// A damaged record blocks startup rather than silently lifting the fence.
pub fn active(home: &HomeLayout) -> io::Result<Option<Intent>> {
    let path = active_path(home);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable_intent(&path, error)),
    };
    let read = || {
        if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
            return Err(invalid("invalid update intent file"));
        }
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(invalid("update intent file exceeds its size limit"));
        }
        let intent: Intent = serde_json::from_slice(&bytes).map_err(invalid)?;
        intent.validate(home)?;
        Ok(Some(intent))
    };
    read().map_err(|error| unreadable_intent(&path, error))
}

fn unreadable_intent(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "update_in_progress: cannot read or validate update intent {}: {error}. Startup remains blocked; after checking the installation and the interrupted update transaction, remove {} manually to allow startup",
            path.display(),
            path.display(),
        ),
    )
}

/// Publish before requesting shutdown. Never evict a dead host's transaction:
/// it may still own the only old executable or an interrupted replacement.
pub fn arm(home: &HomeLayout, intent: &Intent) -> io::Result<()> {
    intent.validate(home)?;
    if NativeHost::capture(std::process::id())? != intent.holder {
        return Err(invalid("update intent belongs to a different host"));
    }
    with_intent_lock(home, || {
        match active(home)? {
            Some(current) if current == *intent => return Ok(()),
            Some(current) => return Err(in_progress(&current)),
            None => {}
        }
        crate::fs::write_json_committed(&active_path(home), intent)
    })
}

/// The returned descriptor, not a ping result, proves exclusive ownership of
/// the stopped daemon's lock. Keep it alive through all apply/abort file work.
/// It closes on exec; the persistent intent protects the subsequent startup gap.
pub fn claim_stopped(home: &HomeLayout, transaction_id: &str) -> io::Result<File> {
    let before = matching(home, transaction_id)?;
    before.require_executor()?;
    let held =
        crate::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "daemon_running: cannot replace or restore files while the daemon lock is held",
            )
        })?;
    let after = matching(home, transaction_id)?;
    if after != before {
        return Err(invalid(
            "update intent changed while claiming the daemon lock",
        ));
    }
    after.require_executor()?;
    Ok(held)
}

/// Call only after installer confirm or successful restoration of the old
/// files. This clears no installer state and never stops any process.
pub fn disarm(home: &HomeLayout, transaction_id: &str) -> io::Result<()> {
    with_intent_lock(home, || {
        let intent = matching(home, transaction_id)?;
        intent.require_executor()?;
        fs::remove_file(active_path(home))?;
        crate::fs::sync_dir(&home.root().join("update"))
    })
}

/// Check immediately before taking the daemon lock and again after taking it,
/// before process-tree cleanup or session restoration can have side effects.
pub fn check_start(
    home: &HomeLayout,
    resume: Option<&Path>,
    actual: &BuildIdentity,
) -> io::Result<()> {
    let Some(intent) = active(home)? else {
        return if resume.is_none() {
            Ok(())
        } else {
            Err(invalid(
                "update_resume_mismatch: no active update transaction",
            ))
        };
    };
    let Some(resume) = resume else {
        return Err(in_progress(&intent));
    };
    actual.validate()?;
    if fs::canonicalize(resume)? != intent.transaction_dir || *actual != intent.target {
        return Err(invalid(
            "update_resume_mismatch: transaction or executable build does not match the prepared update",
        ));
    }
    Ok(())
}

fn matching(home: &HomeLayout, transaction_id: &str) -> io::Result<Intent> {
    validate_id(transaction_id)?;
    match active(home)? {
        Some(intent) if intent.transaction_id == transaction_id => Ok(intent),
        _ => Err(invalid(
            "update transaction does not own this home's startup intent",
        )),
    }
}

fn validate_id(value: &str) -> io::Result<()> {
    let id = uuid::Uuid::parse_str(value).map_err(invalid)?;
    if id.is_nil() || (value != id.simple().to_string() && value != id.hyphenated().to_string()) {
        return Err(invalid("update transaction id must be a non-nil UUID"));
    }
    Ok(())
}

fn transaction_dir(home: &HomeLayout, id: &str) -> io::Result<PathBuf> {
    validate_id(id)?;
    let update = update_dir(home)?;
    let path = update.join(id);
    if fs::symlink_metadata(&path)?.file_type().is_symlink()
        || !path.is_dir()
        || fs::canonicalize(&path)? != path
    {
        return Err(invalid(
            "update transaction must be a real directory under this home",
        ));
    }
    Ok(path)
}

fn update_dir(home: &HomeLayout) -> io::Result<PathBuf> {
    let update = fs::canonicalize(home.root())?.join("update");
    if fs::canonicalize(&update)? != update {
        return Err(invalid("the home's update directory must not be a symlink"));
    }
    Ok(update)
}

fn active_path(home: &HomeLayout) -> PathBuf {
    home.root().join("update/active.json")
}

fn with_intent_lock<T>(home: &HomeLayout, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    crate::fs::with_exclusive_lock(&update_dir(home)?.join("active.lock"), f)
}

fn in_progress(intent: &Intent) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "update_in_progress: transaction {} owns this home's startup; recover with `{}`",
            intent.transaction_id,
            intent.recovery_command()
        ),
    )
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
#[path = "attended_update_tests.rs"]
mod tests;

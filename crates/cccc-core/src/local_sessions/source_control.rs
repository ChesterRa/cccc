//! Durable user choice, separate from lifecycle callbacks and SessionEnd.
use super::{HomeLayout, fs};
use cccc_contracts::source_control::SourceControl;
use sha2::{Digest, Sha256};
use std::{io, path::PathBuf};

fn path(home: &HomeLayout, source_id: &str) -> io::Result<PathBuf> {
    // Validate against the existing opaque native ID grammar; never accept paths.
    super::path(home, source_id)?;
    Ok(home
        .root()
        .join("state/source_controls")
        .join(format!("{:x}.json", Sha256::digest(source_id.as_bytes()))))
}

pub fn state(home: &HomeLayout, source_id: &str) -> io::Result<SourceControl> {
    let path = path(home, source_id)?;
    match fs::read_json(&path) {
        Ok(state) => Ok(state),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(SourceControl::default()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "Could not read state/source_controls/{} ({:?})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                error.kind()
            ),
        )),
    }
}

pub fn blocked(home: &HomeLayout, source_id: &str) -> io::Result<bool> {
    Ok(state(home, source_id)?.access_blocked)
}

/// Serialize with genuine lifecycle writes. Never wait on provider I/O here.
pub fn save(home: &HomeLayout, source_id: &str, state: &SourceControl) -> io::Result<()> {
    super::read_record(home, source_id)?;
    fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
        fs::write_json_committed(&path(home, source_id)?, state)
    })
}

/// Serialize just this management flow (including Group saves), not registration
/// or provider checks. The native lifecycle lock is held only by save().
pub fn with_operation<T>(
    home: &HomeLayout,
    source_id: &str,
    action: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let path = path(home, source_id)?.with_extension("lock");
    fs::with_exclusive_lock(&path, action)
}

use super::{WriteOutcome, digest, resolve_file, resolve_for_create, root};
use crate::GroupDoc;
use std::{fs, io, sync::Mutex};

/// Serializes the digest check in [`write_file`] with the replacement that follows it.
/// Without it, saves carrying the same expected digest all read the same bytes, all pass
/// the check, and every write but the last is silently discarded.
///
/// Process-wide rather than per-path: workspace saves are hand-driven and complete in
/// milliseconds, so a map of per-path locks would only add lifetime bookkeeping. The digest
/// detects external edits completed before the check; writers outside this process do not
/// take this lock and can still race with the replacement.
pub(super) static WRITE_GUARD: Mutex<()> = Mutex::new(());

/// Writes `content`, refusing when the on-disk bytes no longer match `expected_sha256`.
///
/// An empty `expected_sha256` means "the file must not exist at the digest check".
/// This detects an Actor creating the path before that check, not after it.
pub fn write_file(
    group: &GroupDoc,
    relative: &str,
    content: &str,
    expected_sha256: &str,
) -> io::Result<WriteOutcome> {
    let root = root(group)?;
    // Held across the read, the comparison, and the write. A poisoned guard only means an
    // earlier write panicked; this one is still decided on the bytes currently on disk.
    let _guard = WRITE_GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let candidate = resolve_for_create(&root, relative)?;
    let path = match fs::symlink_metadata(&candidate) {
        Ok(_) => resolve_file(group, relative)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => candidate,
        Err(error) => return Err(error),
    };
    let current = match fs::read(&path) {
        Ok(bytes) => Some(digest(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let expected = expected_sha256.trim();
    match (&current, expected.is_empty()) {
        (Some(actual), false) if actual == expected => {}
        (None, true) => {}
        _ => {
            return Ok(WriteOutcome::Conflict {
                sha256: current.unwrap_or_default(),
            });
        }
    }
    let bytes = content.as_bytes();
    // Workspace files belong to the user; an edited script must stay executable.
    crate::fs::atomic_write_preserving_mode(&path, bytes)?;
    Ok(WriteOutcome::Written {
        sha256: digest(bytes),
        created: current.is_none(),
    })
}

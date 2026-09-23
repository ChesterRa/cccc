use std::fs::{self, File};
use std::io;
use std::path::Path;

pub(super) fn move_file(source: &Path, target: &Path) -> io::Result<()> {
    move_with(
        source,
        target,
        |a, b| fs::rename(a, b),
        |path| fs::remove_file(path),
    )
}

// Only rename/unlink are injectable: tests still copy, persist and sync real files.
fn move_with(
    source: &Path,
    target: &Path,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
    remove: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    match rename(source, target) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {}
        Err(error) => return Err(error),
    }
    copy_file(source, target)?;
    // Do not discard the recovery copy if removing the source fails.
    match remove(source) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("{error}; recovery copy retained at {}", target.display()),
        )),
    }
}

// Copy without consuming the backup, so rollback can finish other durable writes first.
pub(super) fn copy_file(source: &Path, target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("target has no parent"))?;
    let mut input = File::open(source)?;
    let before = input.metadata()?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let copied = io::copy(&mut input, temporary.as_file_mut())?;
    let after = input.metadata()?;
    if copied != before.len()
        || after.len() != before.len()
        || after.modified()? != before.modified()?
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "document changed while copying; retry deletion",
        ));
    }
    temporary.as_file().set_permissions(before.permissions())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(target)
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cross_device(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::CrossesDevices.into())
    }

    #[test]
    fn cross_device_move_and_rollback_preserve_complete_contents() {
        let dir = tempfile::tempdir().expect("create fixture directory");
        let source = dir.path().join("source.md");
        let target = dir.path().join("recovery.md");
        let bytes = "原始文档\n".repeat(32_768).into_bytes();
        fs::write(&source, &bytes).expect("write fixture bytes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&source, fs::Permissions::from_mode(0o640))
                .expect("set fixture permissions");
        }
        move_with(&source, &target, cross_device, |p| fs::remove_file(p))
            .expect("remove fixture file");
        assert!(!source.exists());
        assert_eq!(fs::read(&target).expect("read fixture bytes"), bytes);
        move_with(&target, &source, cross_device, |p| fs::remove_file(p))
            .expect("remove fixture file");
        assert!(!target.exists());
        assert_eq!(fs::read(&source).expect("read fixture bytes"), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&source)
                    .expect("read document metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }
    }

    #[test]
    fn failed_copy_publication_keeps_source_and_existing_target() {
        let dir = tempfile::tempdir().expect("create fixture directory");
        let source = dir.path().join("source.md");
        let target = dir.path().join("recovery.md");
        fs::write(&source, "original").expect("write fixture bytes");
        fs::write(&target, "existing recovery").expect("write fixture bytes");
        let error = move_with(&source, &target, cross_device, |_| {
            panic!("must not remove before copy is committed")
        })
        .expect_err("copy publication must reject an existing target");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(source).expect("read document text"),
            "original"
        );
        assert_eq!(
            fs::read_to_string(target).expect("read document text"),
            "existing recovery"
        );
        assert_eq!(
            fs::read_dir(dir.path())
                .expect("read recovery directory")
                .count(),
            2
        );
    }

    #[test]
    fn failed_source_removal_preserves_both_complete_copies() {
        let dir = tempfile::tempdir().expect("create fixture directory");
        let source = dir.path().join("source.md");
        let target = dir.path().join("recovery.md");
        fs::write(&source, "original").expect("write fixture bytes");
        let error = move_with(&source, &target, cross_device, |_| {
            Err(io::ErrorKind::PermissionDenied.into())
        })
        .expect_err("source removal failure must be reported");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            fs::read_to_string(source).expect("read document text"),
            "original"
        );
        assert_eq!(
            fs::read_to_string(target).expect("read document text"),
            "original"
        );
    }
}

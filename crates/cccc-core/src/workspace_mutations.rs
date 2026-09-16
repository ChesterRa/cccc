//! Entry operations share text-save serialization, but never follow the final symlink.
use super::{
    OutsideScope, resolve_for_create, root, safe_relative, to_relative, write::WRITE_GUARD,
};
use crate::GroupDoc;
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

fn entry_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let relative = safe_relative(relative)?;
    if !relative
        .components()
        .any(|part| matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the workspace root cannot be changed",
        ));
    }
    let path = resolve_for_create(
        root,
        relative
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?,
    )?;
    let normalized = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other(OutsideScope))?;
    if normalized.components().any(|part| matches!(part, Component::Normal(name) if name.to_string_lossy().eq_ignore_ascii_case(".git"))) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Git metadata cannot be changed through Files"));
    }
    Ok(path)
}

/// Creates exactly one entry. Native exclusive creation preserves concurrent writers.
pub fn create_entry(group: &GroupDoc, relative: &str, directory: bool) -> io::Result<String> {
    let root = root(group)?;
    let _guard = WRITE_GUARD
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let path = entry_path(&root, relative)?;
    if directory {
        fs::create_dir(&path)?;
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
    }
    Ok(to_relative(&root, &path).unwrap_or_default())
}

/// Rename and drag/drop movement use the same native, no-replace operation.
pub fn move_entry(
    group: &GroupDoc,
    relative: &str,
    destination: &str,
) -> io::Result<(String, String)> {
    let root = root(group)?;
    let _guard = WRITE_GUARD
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let source = entry_path(&root, relative)?;
    let target = entry_path(&root, destination)?;
    let metadata = fs::symlink_metadata(&source)?;
    if source != target {
        if metadata.is_dir() && target.starts_with(&source) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a folder cannot be moved into itself",
            ));
        }
        renamore::rename_exclusive(&source, &target).map_err(|error| {
            if matches!(error.kind(), io::ErrorKind::Unsupported | io::ErrorKind::CrossesDevices) {
                io::Error::new(error.kind(), "This filesystem does not support a non-overwriting move. Use the terminal to move this item.")
            } else { error }
        })?;
    }
    Ok((
        to_relative(&root, &source).unwrap_or_default(),
        to_relative(&root, &target).unwrap_or_default(),
    ))
}

/// A failed recursive delete can have removed some entries; callers must refresh even on error.
pub fn delete_entry(group: &GroupDoc, relative: &str) -> io::Result<String> {
    let root = root(group)?;
    let _guard = WRITE_GUARD
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let path = entry_path(&root, relative)?;
    let kind = fs::symlink_metadata(&path)?.file_type();
    if kind.is_dir() {
        fs::remove_dir_all(&path)?;
    } else {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if kind.is_symlink_dir() {
                fs::remove_dir(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        #[cfg(not(windows))]
        fs::remove_file(&path)?;
    }
    Ok(to_relative(&root, &path).unwrap_or_default())
}

/// Cleanup uses the opened parent, so renaming an ancestor cannot orphan the upload.
struct UploadCleanup {
    directory: fs::File,
    name: OsString,
}
impl Drop for UploadCleanup {
    fn drop(&mut self) {
        if let Err(error) = fs_at::OpenOptions::default().unlink_at(&self.directory, &self.name)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "Could not remove staged workspace upload");
        }
    }
}

/// Stages one streamed upload beside its target. Drop removes interrupted uploads.
/// Publication never overwrites a file and rechecks the parent after a long upload.
pub struct WorkspaceUpload {
    // Close the file before handle-relative cleanup (also required on Windows).
    file: tempfile::NamedTempFile,
    _cleanup: UploadCleanup,
    destination: PathBuf,
    relative: String,
}
impl WorkspaceUpload {
    pub fn new(group: &GroupDoc, relative: &str) -> io::Result<Self> {
        let root = root(group)?;
        let destination = entry_path(&root, relative)?;
        match fs::symlink_metadata(&destination) {
            Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("missing upload parent"))?;
        let mut directory_options = fs::OpenOptions::new();
        directory_options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_BACKUP_SEMANTICS permits opening a directory handle.
            directory_options.custom_flags(0x02000000);
        }
        let directory = directory_options.open(parent)?;
        let mut options = fs_at::OpenOptions::default();
        options
            .read(true)
            .write(fs_at::OpenOptionsWriteMode::Write)
            .create_new(true);
        #[cfg(unix)]
        {
            use fs_at::os::unix::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = tempfile::Builder::new()
            .prefix(".cccc-upload-")
            // UploadCleanup owns deletion. A stale absolute path must never delete a replacement.
            .disable_cleanup(true)
            .make_in(parent, |path| {
                options.open_at(&directory, path.file_name().expect("temporary file name"))
            })?;
        let name = file
            .path()
            .file_name()
            .expect("temporary file name")
            .to_owned();
        Ok(Self {
            file,
            _cleanup: UploadCleanup { directory, name },
            destination,
            relative: relative.to_owned(),
        })
    }
    pub fn writer(&self) -> io::Result<fs::File> {
        self.file.as_file().try_clone()
    }
    pub fn write_chunk(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(bytes)
    }
    pub fn finish(self, group: &GroupDoc) -> io::Result<String> {
        let root = root(group)?;
        let _guard = WRITE_GUARD
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if entry_path(&root, &self.relative)? != self.destination {
            return Err(io::Error::other(
                "upload destination changed; select the folder again",
            ));
        }
        // The path could have been recreated after a parent move. Do not publish its replacement.
        if same_file::Handle::from_file(self.file.as_file().try_clone()?)?
            != same_file::Handle::from_path(self.file.path())?
        {
            return Err(io::Error::other(
                "upload destination changed; select the folder again",
            ));
        }
        self.file.as_file().sync_all()?;
        self.file
            .persist_noclobber(&self.destination)
            .map_err(|error| error.error)?;
        Ok(to_relative(&root, &self.destination).unwrap_or_default())
    }
}

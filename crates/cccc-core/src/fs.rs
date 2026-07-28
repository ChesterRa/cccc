use fs2::FileExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    sync_dir(parent)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub fn write_json_committed<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    match write_json(path, value) {
        Ok(()) => Ok(()),
        Err(error) => match read_json::<T>(path) {
            Ok(actual) if actual == *value => Ok(()),
            _ => Err(error),
        },
    }
}

pub fn write_secret_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_json(path, value)?;
    protect(path)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_yaml::to_string(value).map_err(io::Error::other)?;
    atomic_write(path, text.as_bytes())
}

pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_yaml::from_reader(File::open(path)?).map_err(io::Error::other)
}

pub fn with_exclusive_lock<T>(
    path: &Path,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock_exclusive()?;
    let result = operation();
    // The descriptor is dropped on return and releases the advisory lock even if an explicit
    // unlock reports an OS-level error. Do not turn a committed operation into an ambiguous
    // failure after its durable write has already completed.
    let _ = FileExt::unlock(&file);
    result
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(unix)]
fn protect(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn protect(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

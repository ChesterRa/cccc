use serde::Serialize;
use serde::de::DeserializeOwned;
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

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_reader(File::open(path)?).map_err(io::Error::other)
}

pub fn write_yaml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_yaml::to_string(value).map_err(io::Error::other)?;
    atomic_write(path, text.as_bytes())
}

pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_yaml::from_reader(File::open(path)?).map_err(io::Error::other)
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

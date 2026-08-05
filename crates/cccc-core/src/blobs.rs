use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BlobInfo {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

pub fn store(home: &HomeLayout, group_id: &str, data: &[u8]) -> io::Result<BlobInfo> {
    let digest = format!("{:x}", Sha256::digest(data));
    let state = GroupStore::new(home.clone())?.state_dir(group_id)?;
    let relative = format!("state/blobs/{digest}");
    let path = state.join("blobs").join(&digest);
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("invalid blob path"))?,
    )?;
    if !path.exists() {
        crate::fs::atomic_write(&path, data)?;
    }
    Ok(BlobInfo {
        path: relative,
        bytes: data.len(),
        sha256: digest,
    })
}

pub struct BlobUpload {
    file: tempfile::NamedTempFile,
    blobs_dir: PathBuf,
    hasher: Sha256,
    bytes: usize,
}

impl BlobUpload {
    pub fn new(home: &HomeLayout, group_id: &str) -> io::Result<Self> {
        let blobs_dir = GroupStore::new(home.clone())?
            .state_dir(group_id)?
            .join("blobs");
        fs::create_dir_all(&blobs_dir)?;
        Ok(Self {
            file: tempfile::NamedTempFile::new_in(&blobs_dir)?,
            blobs_dir,
            hasher: Sha256::new(),
            bytes: 0,
        })
    }

    pub fn write_chunk(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)?;
        self.hasher.update(data);
        self.bytes += data.len();
        Ok(())
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn finish(self) -> io::Result<BlobInfo> {
        self.file.as_file().sync_all()?;
        let digest = format!("{:x}", self.hasher.finalize());
        let path = self.blobs_dir.join(&digest);
        if !path.exists()
            && let Err(error) = self.file.persist_noclobber(&path)
            && error.error.kind() != io::ErrorKind::AlreadyExists
        {
            return Err(error.error);
        }
        Ok(BlobInfo {
            path: format!("state/blobs/{digest}"),
            bytes: self.bytes,
            sha256: digest,
        })
    }
}

pub fn resolve(home: &HomeLayout, group_id: &str, relative: &str) -> io::Result<PathBuf> {
    let name = relative.strip_prefix("state/blobs/").unwrap_or(relative);
    if !valid_blob_name(name) {
        return Err(io::Error::other("invalid blob path"));
    }
    let blobs = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("blobs");
    let path = blobs.join(name);
    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "blob not found"));
    }
    let base = fs::canonicalize(blobs)?;
    let resolved = fs::canonicalize(path)?;
    resolved
        .starts_with(&base)
        .then_some(resolved)
        .ok_or_else(|| io::Error::other("invalid blob path"))
}

fn valid_blob_name(name: &str) -> bool {
    if name.len() < 64 || name.len() > 185 {
        return false;
    }
    let (digest, suffix) = name.split_at(64);
    if !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    if suffix.is_empty() {
        return true;
    }
    let Some(filename) = suffix.strip_prefix('_') else {
        return false;
    };
    !filename.is_empty()
        && filename.len() <= 120
        && !filename.contains("..")
        && filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && filename.bytes().any(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_python_and_rust_blob_names_without_path_escape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("blob compatibility", "").expect("group");
        let digest = "a".repeat(64);
        let legacy_name = format!("{digest}_image.png");
        let legacy_path = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("blobs")
            .join(&legacy_name);
        fs::write(&legacy_path, b"legacy image").expect("legacy blob");

        assert_eq!(
            resolve(&home, &group.group_id, &legacy_name).expect("legacy resolve"),
            fs::canonicalize(legacy_path).expect("canonical legacy path")
        );
        assert!(valid_blob_name(&digest));
        assert!(!valid_blob_name(&format!("{digest}_../secret")));
        assert!(!valid_blob_name(&format!("{digest}_image/name.png")));
        assert!(!valid_blob_name("not-a-digest_image.png"));
    }

    #[test]
    fn streamed_upload_hashes_chunks_and_persists_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("streamed blob", "").expect("group");
        let mut upload = BlobUpload::new(&home, &group.group_id).expect("upload");
        upload.write_chunk(b"hello ").expect("first chunk");
        upload.write_chunk(b"world").expect("second chunk");
        assert_eq!(upload.bytes(), 11);
        let info = upload.finish().expect("finish");
        assert_eq!(
            fs::read(resolve(&home, &group.group_id, &info.path).expect("resolve")).expect("read"),
            b"hello world"
        );
        assert_eq!(info.sha256, format!("{:x}", Sha256::digest(b"hello world")));
    }
}

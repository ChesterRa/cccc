use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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

/// Hash a stored blob without loading it into memory.
pub fn digest_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

/// Place a blob held elsewhere into `group_id` under the same name. Entries are
/// immutable, so an existing one is kept. A hard link shares the bytes; when
/// linking is impossible a streamed copy is re-hashed before it is visible.
pub fn place(
    home: &HomeLayout,
    group_id: &str,
    name: &str,
    source: &Path,
    sha256: &str,
) -> io::Result<String> {
    if !valid_blob_name(name) {
        return Err(io::Error::other("invalid blob path"));
    }
    let relative = format!("state/blobs/{name}");
    let blobs_dir = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("blobs");
    fs::create_dir_all(&blobs_dir)?;
    let target = blobs_dir.join(name);
    let verify_existing = || -> io::Result<()> {
        // An existing name is reusable only when it resolves inside this
        // Group and carries the verified source bytes. Never overwrite it.
        let stored = resolve(home, group_id, &relative)?;
        if fs::metadata(&stored)?.len() != fs::metadata(source)?.len()
            || digest_file(&stored)?.0 != sha256
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "existing destination blob does not match the source",
            ));
        }
        Ok(())
    };
    if target.is_file() {
        verify_existing()?;
        return Ok(relative);
    }
    match fs::hard_link(source, &target) {
        Ok(()) => return Ok(relative),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_existing()?;
            return Ok(relative);
        }
        Err(_) => {}
    }
    let mut copy = BlobUpload {
        file: tempfile::NamedTempFile::new_in(&blobs_dir)?,
        blobs_dir,
        hasher: Sha256::new(),
        bytes: 0,
    };
    let mut file = fs::File::open(source)?;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copy.write_chunk(&buffer[..read])?;
    }
    copy.file.as_file().sync_all()?;
    if format!("{:x}", copy.hasher.finalize()) != sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "blob changed while it was copied",
        ));
    }
    if let Err(error) = copy.file.persist_noclobber(&target) {
        if error.error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.error);
        }
        verify_existing()?;
    }
    Ok(relative)
}

fn valid_blob_name(name: &str) -> bool {
    if name.len() < 64 || name.len() > 185 {
        return false;
    }
    let Some((digest, suffix)) = name.split_at_checked(64) else {
        return false;
    };
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
        assert!(!valid_blob_name(&format!("{}é", "a".repeat(63))));
        assert!(!valid_blob_name(&format!("{}🟦", "a".repeat(61))));
    }

    #[test]
    fn placing_a_blob_shares_it_with_another_group_idempotently() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let source = store.create("source", "").expect("source");
        let target = store.create("target", "").expect("target");
        let info = super::store(&home, &source.group_id, b"shared bytes").expect("blob");
        let path = resolve(&home, &source.group_id, &info.path).expect("source path");
        assert_eq!(
            digest_file(&path).expect("digest"),
            (info.sha256.clone(), 12)
        );
        assert!(resolve(&home, &target.group_id, &info.path).is_err());
        for _ in 0..2 {
            let placed =
                place(&home, &target.group_id, &info.sha256, &path, &info.sha256).expect("place");
            assert_eq!(placed, info.path);
        }
        assert_eq!(
            fs::read(resolve(&home, &target.group_id, &info.path).expect("target")).expect("read"),
            b"shared bytes"
        );
        assert!(place(&home, &target.group_id, "../escape", &path, &info.sha256).is_err());
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

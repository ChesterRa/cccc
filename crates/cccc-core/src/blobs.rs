use sha2::{Digest, Sha256};
use std::fs;
use std::io;
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

pub fn resolve(home: &HomeLayout, group_id: &str, relative: &str) -> io::Result<PathBuf> {
    let name = relative.strip_prefix("state/blobs/").unwrap_or(relative);
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("invalid blob path"));
    }
    let path = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("blobs")
        .join(name);
    path.exists()
        .then_some(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "blob not found"))
}

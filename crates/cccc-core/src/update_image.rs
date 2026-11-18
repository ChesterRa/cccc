//! File identity for an explicitly requested switch to a rebuilt executable.
//! It is not the application's Rust-source fingerprint or an authority token.

use cccc_runtime::running_image::ImageFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub written: SystemTime,
}

impl FileIdentity {
    /// Follow the launch path to the file currently there. Call before and
    /// after inspecting the candidate so a concurrent rebuild is detected.
    pub fn read(path: &Path) -> io::Result<Self> {
        ImageFile::at(path).map(Self::from)
    }

    /// The image this process is executing, even when its former launch path
    /// now names another file.
    pub fn running() -> io::Result<Self> {
        ImageFile::running().map(Self::from)
    }

    /// Opaque comparison value for the check/confirm round trip. This grants
    /// no permission: the host always rereads the actual candidate file.
    pub fn token(&self) -> io::Result<String> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

impl From<ImageFile> for FileIdentity {
    fn from(file: ImageFile) -> Self {
        Self {
            device: file.device,
            inode: file.inode,
            size: file.size,
            written: file.written,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_same_size_replacement_changes_identity_and_confirmation_token() {
        let temp = tempfile::tempdir().expect("isolated files");
        let path = temp.path().join("cccc");
        std::fs::write(&path, "old").expect("old file");
        let old = FileIdentity::read(&path).expect("old identity");
        let replacement = temp.path().join("next");
        std::fs::write(&replacement, "new").expect("same size replacement");
        std::fs::rename(replacement, &path).expect("replace as a build does");
        let new = FileIdentity::read(&path).expect("new identity");
        assert_eq!(old.size, new.size);
        assert_ne!(old, new);
        assert_ne!(
            old.token().expect("old token"),
            new.token().expect("new token")
        );
        assert_eq!(new, FileIdentity::read(&path).expect("unchanged identity"));
        assert!(FileIdentity::read(temp.path()).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn running_image_uses_the_kernel_image_not_a_caller_supplied_path() {
        let actual = FileIdentity::running().expect("kernel image");
        assert_eq!(
            actual,
            FileIdentity::read(&std::env::current_exe().expect("test image path"))
                .expect("image on disk")
        );
        let encoded = serde_json::to_string(&actual).expect("intent representation");
        assert_eq!(
            actual,
            serde_json::from_str::<FileIdentity>(&encoded).expect("round trip")
        );
    }
}

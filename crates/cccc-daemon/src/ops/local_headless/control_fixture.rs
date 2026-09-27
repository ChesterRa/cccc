//! A fake Claude Agent View control socket at the path its client derives from a config dir.

use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub(super) struct ControlDirectory(PathBuf);

impl Drop for ControlDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn bind(config: &Path) -> (tokio::net::UnixListener, ControlDirectory) {
    let digest = format!("{:x}", Sha256::digest(config.to_string_lossy().as_bytes()));
    let directory = PathBuf::from("/tmp")
        .join(format!(
            "cc-daemon-{}",
            std::fs::metadata(config).expect("metadata").uid()
        ))
        .join(&digest[..8]);
    std::fs::create_dir_all(&directory).expect("control directory");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .expect("permissions");
    let listener = tokio::net::UnixListener::bind(directory.join("control.sock")).expect("socket");
    (listener, ControlDirectory(directory))
}

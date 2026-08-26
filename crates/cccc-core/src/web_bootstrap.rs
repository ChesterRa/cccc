use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use uuid::Uuid;

use crate::HomeLayout;
use crate::access_tokens::AccessTokenStore;

pub const WEB_BOOTSTRAP_TOKEN_FILENAME: &str = "web_bootstrap_token";

pub fn ensure_web_bootstrap_token(home: &HomeLayout) -> io::Result<Option<PathBuf>> {
    let store = AccessTokenStore::new(home.clone())?;
    if store.list()?.iter().any(|token| token.is_admin) {
        let path = bootstrap_path(home);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(None);
    }

    let path = bootstrap_path(home);
    if valid_stored_token(&path)? {
        repair_or_remove(&path)?;
        return Ok(Some(path));
    }
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let token = format!("boot_{}", Uuid::new_v4().simple());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(token.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            repair_or_remove(&path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    Ok(Some(path))
}

fn repair_or_remove(path: &std::path::Path) -> io::Result<()> {
    if let Err(error) = protect(path) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

pub fn consume_web_bootstrap_token(home: &HomeLayout, provided: &str) -> io::Result<bool> {
    let Some(path) = ensure_web_bootstrap_token(home)? else {
        return Ok(false);
    };
    let expected = std::fs::read_to_string(&path)?.trim().to_owned();
    let candidate = provided.trim();
    if candidate.is_empty() || token_digest(candidate) != token_digest(&expected) {
        return Ok(false);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn bootstrap_path(home: &HomeLayout) -> PathBuf {
    home.root().join(WEB_BOOTSTRAP_TOKEN_FILENAME)
}

fn valid_stored_token(path: &std::path::Path) -> io::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    Ok(std::fs::read_to_string(path)?.trim().starts_with("boot_"))
}

fn token_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

#[cfg(unix)]
fn protect(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn protect(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_token_is_one_time_and_removed_after_admin_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let path = ensure_web_bootstrap_token(&home)
            .expect("ensure")
            .expect("bootstrap path");
        let secret = std::fs::read_to_string(&path).expect("secret");
        assert!(!consume_web_bootstrap_token(&home, "wrong").expect("wrong"));
        assert!(consume_web_bootstrap_token(&home, &secret).expect("consume"));
        assert!(!path.exists());

        AccessTokenStore::new(home.clone())
            .expect("store")
            .create("admin", Vec::new(), true, None)
            .expect("admin");
        assert!(
            ensure_web_bootstrap_token(&home)
                .expect("cleanup")
                .is_none()
        );
        assert!(!path.exists());
    }
}

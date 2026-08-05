use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Scope;

pub fn detect(path: &Path) -> io::Result<Scope> {
    let absolute = path.canonicalize()?;
    let root = git_output(&absolute, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .unwrap_or_else(|| absolute.clone());
    let remote = git_output(&root, &["remote", "get-url", "origin"])
        .map(|value| normalize_remote(&value))
        .unwrap_or_default();
    let url = root.to_string_lossy().into_owned();
    let seed = if remote.is_empty() { &url } else { &remote };
    let digest = Sha256::digest(seed.as_bytes());
    Ok(Scope {
        scope_key: format!("s_{digest:x}")[..14].to_owned(),
        url,
        label: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("scope")
            .into(),
        git_remote: remote,
    })
}

pub fn normalize_remote(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if let Some((user_host, path)) = value.split_once(':')
        && let Some((user, host)) = user_host.split_once('@')
        && !user.is_empty()
        && !host.is_empty()
        && !host.contains('/')
        && !path.is_empty()
    {
        return format!(
            "https://{host}/{}",
            path.strip_suffix(".git").unwrap_or(path)
        );
    }
    if let Some(value) = value.strip_prefix("ssh://") {
        let value = value.replacen("git@", "", 1);
        if let Some((host, path)) = value.split_once('/') {
            return format!(
                "https://{host}/{}",
                path.strip_suffix(".git").unwrap_or(path)
            );
        }
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.strip_suffix(".git").unwrap_or(value).to_owned();
    }
    value.to_owned()
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_normalization_matches_python_identity_contract() {
        for (input, expected) in [
            (
                "git@github.com:Acme/Repo.git",
                "https://github.com/Acme/Repo",
            ),
            (
                "ssh://git@github.com/Acme/Repo.git",
                "https://github.com/Acme/Repo",
            ),
            (
                "https://github.com/Acme/Repo.git",
                "https://github.com/Acme/Repo",
            ),
            ("file:///tmp/Repo.git", "file:///tmp/Repo.git"),
        ] {
            assert_eq!(normalize_remote(input), expected);
        }
    }

    #[test]
    fn detected_scope_returns_the_normalized_remote_and_python_compatible_key() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(temp.path())
                .status()
                .expect("git init")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["remote", "add", "origin", "git@github.com:Acme/Repo.git",])
                .current_dir(temp.path())
                .status()
                .expect("git remote")
                .success()
        );

        let scope = detect(temp.path()).expect("scope");

        assert_eq!(scope.git_remote, "https://github.com/Acme/Repo");
        assert_eq!(scope.scope_key, "s_3b1f782ba15e");
    }
}

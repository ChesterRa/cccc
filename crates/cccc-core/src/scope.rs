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
    let remote = git_output(&root, &["remote", "get-url", "origin"]).unwrap_or_default();
    let url = root.to_string_lossy().into_owned();
    let seed = if remote.is_empty() { &url } else { &remote };
    let digest = Sha256::digest(normalize_remote(seed).as_bytes());
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
    let mut normalized = value
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_lowercase();
    if let Some((host, path)) = normalized.split_once(':')
        && !host.contains('/')
        && host.contains('@')
    {
        normalized = format!("ssh://{host}/{path}");
    }
    normalized
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

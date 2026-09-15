//! Group workspace file access confined to the active scope root.
//!
//! Every path that reaches the filesystem goes through [`safe_relative`] and a
//! canonicalized `starts_with(root)` check, so symlinks and `..` cannot escape the
//! group's active scope. `presentation` reuses these primitives so the boundary has
//! exactly one implementation.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
#[path = "workspace_write.rs"]
mod write;
pub use write::write_file;

use crate::GroupDoc;
use crate::workspace_git::{self, GitStatus};

/// Files larger than this are reported with `truncated` instead of streamed into JSON.
pub const MAX_READ_BYTES: u64 = 1_048_576;

const READ_SNIFF_BYTES: usize = 8_192;

#[derive(Debug, Clone, Copy, Default)]
pub struct ListOptions {
    /// Include entries matched by `.gitignore`.
    pub show_ignored: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitStatus>,
    /// A directory whose subtree contains changes, even when the directory itself is clean.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub git_dirty_descendant: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ignored: bool,
}

#[derive(Debug, Clone)]
pub struct Listing {
    pub root: PathBuf,
    pub path: String,
    pub parent: Option<String>,
    pub items: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileContent {
    pub scope_key: String,
    pub scope_url: String,
    pub path: String,
    pub content: String,
    pub bytes: u64,
    pub mime_type: String,
    pub binary: bool,
    pub truncated: bool,
    /// Digest of the on-disk bytes; the caller echoes it back on write to detect conflicts.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Written {
        sha256: String,
        created: bool,
    },
    /// The file changed since the caller read it; `sha256` is the current on-disk digest.
    Conflict {
        sha256: String,
    },
}

/// The active attachment supplies the workspace location and identity.
fn active_scope(group: &GroupDoc) -> io::Result<&crate::Scope> {
    group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .ok_or_else(|| io::Error::other("group has no active scope"))
}

/// Absolute, canonicalized root of the group's active scope.
pub fn root(group: &GroupDoc) -> io::Result<PathBuf> {
    Path::new(&active_scope(group)?.url).canonicalize()
}

/// Rejects absolute paths and any component that could climb out of the root.
pub fn safe_relative(value: &str) -> io::Result<PathBuf> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(io::Error::other("path must stay under the active scope"))
    } else {
        Ok(path.into())
    }
}

/// Canonicalizes an existing path under `root` and proves it did not escape.
pub fn resolve_existing(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let candidate = root.join(safe_relative(relative)?).canonicalize()?;
    if candidate.starts_with(root) {
        Ok(candidate)
    } else {
        Err(io::Error::other("path must stay under the active scope"))
    }
}

pub fn resolve_file(group: &GroupDoc, relative: &str) -> io::Result<PathBuf> {
    let path = resolve_existing(&root(group)?, relative)?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(io::Error::other(
            "path must be a file under the active scope",
        ))
    }
}

/// Resolves a path that may not exist yet by canonicalizing its parent directory.
pub fn resolve_for_create(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let relative = safe_relative(relative)?;
    let name = relative
        .file_name()
        .ok_or_else(|| io::Error::other("path must name a file"))?;
    let parent = match relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => resolve_existing(root, &parent.to_string_lossy())?,
        None => root.to_path_buf(),
    };
    if !parent.is_dir() {
        return Err(io::Error::other(
            "parent must be a directory under the active scope",
        ));
    }
    Ok(parent.join(name))
}

pub fn list(group: &GroupDoc, relative: &str, options: ListOptions) -> io::Result<Listing> {
    let root = root(group)?;
    let (directory, normalized) = if relative.is_empty() {
        (root.clone(), String::new())
    } else {
        let resolved = resolve_existing(&root, relative)?;
        let normalized = to_relative(&root, &resolved).unwrap_or_default();
        (resolved, normalized)
    };
    if !directory.is_dir() {
        return Err(io::Error::other(
            "workspace path must be a directory under the active scope",
        ));
    }

    let mut entries = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let relative = to_relative(&root, &path)?;
            // `.git` is plumbing, never a browsable part of the workspace.
            if relative == ".git" || relative.starts_with(".git/") {
                return None;
            }
            let is_dir = path.is_dir();
            Some(Entry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: relative,
                is_dir,
                mime_type: (!is_dir).then(|| {
                    mime_guess::from_path(&path)
                        .first_or_octet_stream()
                        .to_string()
                }),
                size: (!is_dir)
                    .then(|| entry.metadata().ok().map(|meta| meta.len()))
                    .flatten(),
                git_status: None,
                git_dirty_descendant: false,
                ignored: false,
            })
        })
        .collect::<Vec<_>>();

    let candidates: Vec<String> = entries.iter().map(|entry| entry.path.clone()).collect();
    let ignored = workspace_git::ignored(&root, &candidates);
    for entry in &mut entries {
        entry.ignored = ignored.contains(&entry.path);
    }
    if !options.show_ignored {
        entries.retain(|entry| !entry.ignored);
    }

    let status = workspace_git::status_map(&root, &normalized);
    for entry in &mut entries {
        entry.git_status = status.get(&entry.path).copied();
        if entry.is_dir {
            let prefix = format!("{}/", entry.path);
            entry.git_dirty_descendant = entry.git_status.is_none()
                && status.keys().any(|changed| changed.starts_with(&prefix));
        }
    }

    entries.sort_by(|left, right| {
        (!left.is_dir, left.name.to_ascii_lowercase())
            .cmp(&(!right.is_dir, right.name.to_ascii_lowercase()))
    });

    let parent = Path::new(&normalized)
        .parent()
        .map(|path| {
            path.to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .filter(|path| !path.is_empty())
        .or_else(|| (!normalized.is_empty()).then(String::new));

    Ok(Listing {
        root,
        path: normalized,
        parent,
        items: entries,
    })
}

pub fn read_file(group: &GroupDoc, relative: &str) -> io::Result<FileContent> {
    let scope = active_scope(group)?;
    let root = root(group)?;
    let path = resolve_existing(&root, relative)?;
    if !path.is_file() {
        return Err(io::Error::other(
            "path must be a file under the active scope",
        ));
    }
    let normalized = to_relative(&root, &path).unwrap_or_else(|| relative.to_owned());
    let mime_type = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    let bytes = fs::metadata(&path)?.len();
    if bytes > MAX_READ_BYTES {
        return Ok(FileContent {
            scope_key: group.active_scope_key.clone(),
            scope_url: scope.url.clone(),
            path: normalized,
            content: String::new(),
            bytes,
            mime_type,
            binary: false,
            truncated: true,
            sha256: String::new(),
        });
    }
    let raw = fs::read(&path)?;
    let binary = is_binary(&raw);
    Ok(FileContent {
        scope_key: group.active_scope_key.clone(),
        scope_url: scope.url.clone(),
        path: normalized,
        sha256: digest(&raw),
        content: if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&raw).into_owned()
        },
        bytes,
        mime_type,
        binary,
        truncated: false,
    })
}

fn to_relative(root: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    )
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(READ_SNIFF_BYTES)];
    head.contains(&0) || std::str::from_utf8(bytes).is_err()
}

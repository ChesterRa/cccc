//! Git decorations for the workspace file tree.
//!
//! Everything here degrades to "no information" when the workspace is not a git
//! repository or the `git` binary is unavailable, so browsing never depends on git.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const QUERY_BUDGET: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

/// Working-tree status keyed by path relative to the workspace root.
///
/// `scope` is the workspace-relative directory being listed (empty for the root). A listing
/// only needs badges for its own entries and rollups for their subtrees, so the walk is limited
/// to that subtree instead of the whole repository.
pub fn status_map(root: &Path, scope: &str) -> BTreeMap<String, GitStatus> {
    status_map_before(root, scope, Instant::now() + QUERY_BUDGET)
}

pub(crate) fn decorations(
    root: &Path,
    scope: &str,
    candidates: &[String],
) -> (BTreeSet<String>, BTreeMap<String, GitStatus>) {
    let deadline = Instant::now() + QUERY_BUDGET;
    let ignored = ignored_before(root, candidates, deadline);
    (ignored, status_map_before(root, scope, deadline))
}

fn status_map_before(root: &Path, scope: &str, deadline: Instant) -> BTreeMap<String, GitStatus> {
    let Some(prefix) = repo_prefix(root, deadline) else {
        return BTreeMap::new();
    };
    let pathspec = status_pathspec(scope);
    let Some(stdout) = git_stdout(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--",
            &pathspec,
        ],
        None,
        deadline,
    ) else {
        return BTreeMap::new();
    };
    parse_status(&stdout, &prefix)
}

/// `"."` keeps the walk inside the workspace root even when the root sits below the repository
/// root; a named directory narrows it further.
fn status_pathspec(scope: &str) -> String {
    let scope = scope.trim_matches('/');
    if scope.is_empty() {
        ".".to_owned()
    } else {
        format!(":(literal){scope}")
    }
}

/// Subset of `candidates` (workspace-relative paths) matched by `.gitignore` rules.
pub fn ignored(root: &Path, candidates: &[String]) -> BTreeSet<String> {
    ignored_before(root, candidates, Instant::now() + QUERY_BUDGET)
}

fn ignored_before(root: &Path, candidates: &[String], deadline: Instant) -> BTreeSet<String> {
    if candidates.is_empty() {
        return BTreeSet::new();
    }
    let mut input = candidates.join("\0");
    input.push('\0');
    let Some(stdout) = git_stdout(
        root,
        &["check-ignore", "-z", "--stdin"],
        Some(input.as_bytes()),
        deadline,
    ) else {
        return BTreeSet::new();
    };
    stdout
        .split('\0')
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_owned())
        .collect()
}

/// Path of `root` relative to the repository root, with a trailing slash (empty at the top).
fn repo_prefix(root: &Path, deadline: Instant) -> Option<String> {
    git_stdout(root, &["rev-parse", "--show-prefix"], None, deadline)
        .map(|value| value.trim_end_matches(['\n', '\r']).to_owned())
}

fn parse_status(stdout: &str, prefix: &str) -> BTreeMap<String, GitStatus> {
    let mut entries = BTreeMap::new();
    let mut tokens = stdout.split('\0').filter(|token| !token.is_empty());
    while let Some(token) = tokens.next() {
        if token.len() < 4 {
            continue;
        }
        let (code, path) = token.split_at(2);
        let status = classify(code);
        // Rename and copy records carry their source path in the next NUL-separated token.
        if matches!(status, GitStatus::Renamed) {
            tokens.next();
        }
        let Some(relative) = path
            .strip_prefix(' ')
            .and_then(|path| path.strip_prefix(prefix))
        else {
            continue;
        };
        let relative = relative.trim_end_matches('/');
        if !relative.is_empty() {
            entries.insert(relative.to_owned(), status);
        }
    }
    entries
}

fn classify(code: &str) -> GitStatus {
    let mut chars = code.chars();
    let index = chars.next().unwrap_or(' ');
    let tree = chars.next().unwrap_or(' ');
    if code == "??" {
        return GitStatus::Untracked;
    }
    if index == 'U' || tree == 'U' || code == "AA" || code == "DD" {
        return GitStatus::Conflicted;
    }
    if index == 'R' || tree == 'R' || index == 'C' || tree == 'C' {
        return GitStatus::Renamed;
    }
    if index == 'A' || tree == 'A' {
        return GitStatus::Added;
    }
    if index == 'D' || tree == 'D' {
        return GitStatus::Deleted;
    }
    GitStatus::Modified
}

fn git_stdout(
    cwd: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
    deadline: Instant,
) -> Option<String> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    // Git is optional decoration. Reuse finite-command ownership so a stuck
    // fsmonitor (including descendants holding pipes open) cannot trap browsing.
    let output = cccc_runtime::capture_command_blocking(
        Command::new("git")
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .current_dir(cwd),
        stdin,
        remaining,
        8 * 1024 * 1024,
    )
    .ok()?;
    if output.stdout_truncated {
        return None;
    }
    // `check-ignore` exits 1 when nothing matched, which is a valid empty answer.
    if !output.status.success() && output.stdout.is_empty() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_tree_badges() {
        let stdout = " M src/lib.rs\0?? notes/\0A  added.rs\0 D gone.rs\0UU merge.rs\0";
        let parsed = parse_status(stdout, "");
        assert_eq!(parsed["src/lib.rs"], GitStatus::Modified);
        assert_eq!(parsed["notes"], GitStatus::Untracked);
        assert_eq!(parsed["added.rs"], GitStatus::Added);
        assert_eq!(parsed["gone.rs"], GitStatus::Deleted);
        assert_eq!(parsed["merge.rs"], GitStatus::Conflicted);
    }

    #[test]
    fn status_paths_preserve_whitespace_in_file_names() {
        let parsed = parse_status(
            " M  leading.txt\0?? trailing.txt \0 M sub/ nested.txt \0",
            "",
        );
        assert_eq!(parsed[" leading.txt"], GitStatus::Modified);
        assert_eq!(parsed["trailing.txt "], GitStatus::Untracked);
        assert_eq!(parsed["sub/ nested.txt "], GitStatus::Modified);
        let nested = parse_status(" M sub/ nested.txt \0", "sub/");
        assert_eq!(nested[" nested.txt "], GitStatus::Modified);
    }

    #[test]
    fn rename_records_consume_their_source_path() {
        // `R  new\0old` must not leave `old` behind as a bogus entry.
        let stdout = "R  docs/new.md\0docs/old.md\0 M keep.rs\0";
        let parsed = parse_status(stdout, "");
        assert_eq!(parsed["docs/new.md"], GitStatus::Renamed);
        assert_eq!(parsed["keep.rs"], GitStatus::Modified);
        assert!(!parsed.contains_key("docs/old.md"));
    }

    #[test]
    fn the_status_walk_is_limited_to_the_directory_being_listed() {
        // The root listing still needs the whole workspace for its dirty-descendant rollups,
        // but expanding a directory must not re-walk the repository.
        assert_eq!(status_pathspec(""), ".");
        assert_eq!(status_pathspec("   "), ":(literal)   ");
        assert_eq!(
            status_pathspec("crates/cccc-web/src"),
            ":(literal)crates/cccc-web/src"
        );
        assert_eq!(status_pathspec("/crates/"), ":(literal)crates");
    }

    #[test]
    fn paths_outside_the_workspace_prefix_are_dropped() {
        // The scope root can sit below the repository root; porcelain paths are repo-relative.
        let stdout = " M sub/dir/kept.rs\0 M other/skipped.rs\0";
        let parsed = parse_status(stdout, "sub/dir/");
        assert_eq!(parsed["kept.rs"], GitStatus::Modified);
        assert_eq!(parsed.len(), 1);
    }
}

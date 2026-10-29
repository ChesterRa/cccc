//! Read-only, bounded Git inspection for one Group workspace.
use crate::{GroupDoc, workspace};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(3);
const STATUS_LIMIT: usize = 2 * 1024 * 1024;
const PATCH_LIMIT: usize = 1024 * 1024;
const ENTRY_LIMIT: usize = 2000;

#[derive(Debug, Serialize)]
pub struct Change {
    pub path: String,
    pub index: char,
    pub worktree: char,
    pub previous_path: Option<String>,
    pub untracked: bool,
    pub conflicted: bool,
    pub directory: bool,
}
#[derive(Debug, Serialize)]
pub struct Changes {
    pub repository: bool,
    pub branch: String,
    pub entries: Vec<Change>,
    pub limited: bool,
}
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffSide {
    Worktree,
    Staged,
}
#[derive(Debug, Serialize)]
pub struct Patch {
    pub patch: String,
    pub limited: bool,
}

fn run(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    limit: usize,
) -> io::Result<cccc_runtime::CapturedOutput> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Git inspection timed out"))?;
    cccc_runtime::capture_command_blocking(
        Command::new("git")
            .args([
                "--no-pager",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.quotePath=false",
            ])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(root),
        None,
        remaining,
        limit,
    )
}
fn text(output: cccc_runtime::CapturedOutput) -> io::Result<String> {
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    if output.stdout_truncated {
        return Err(io::Error::other(
            "Git status exceeds the inspection limit; use the terminal to inspect this repository",
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| {
        io::Error::other(
            "Git returned a path or patch that is not valid UTF-8; use the terminal to inspect it",
        )
    })
}
fn parse(stdout: &str, prefix: &str) -> io::Result<Vec<Change>> {
    let mut tokens = stdout.split('\0').filter(|value| !value.is_empty());
    let mut entries = Vec::new();
    while let Some(token) = tokens.next() {
        let bytes = token.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' || !bytes[0].is_ascii() || !bytes[1].is_ascii() {
            return Err(io::Error::other("Git returned an invalid status record"));
        }
        let index = bytes[0] as char;
        let worktree = bytes[1] as char;
        let previous = if [index, worktree]
            .iter()
            .any(|code| matches!(code, 'R' | 'C'))
        {
            Some(
                tokens
                    .next()
                    .ok_or_else(|| io::Error::other("Git returned an incomplete rename record"))?,
            )
        } else {
            None
        };
        let Some(path) = token[3..].strip_prefix(prefix) else {
            continue;
        };
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            continue;
        }
        workspace::safe_relative(path)?;
        entries.push(Change {
            path: path.to_owned(),
            index,
            worktree,
            previous_path: previous
                .and_then(|path| path.strip_prefix(prefix))
                .map(str::to_owned),
            untracked: index == '?' && worktree == '?',
            conflicted: index == 'U'
                || worktree == 'U'
                || (index == worktree && matches!(index, 'A' | 'D')),
            directory: token.ends_with('/'),
        });
    }
    Ok(entries)
}
/// Also reports whether the workspace is itself a new directory of its repository.
fn inspect(
    root: &Path,
    deadline: Instant,
    limit: usize,
    expand_untracked: bool,
) -> io::Result<(Changes, bool)> {
    let repository = run(
        root,
        &["rev-parse", "--is-inside-work-tree"],
        deadline,
        4096,
    )?;
    if !repository.status.success() {
        // A non-repository is a normal Files state; ownership/permission failures are not.
        if String::from_utf8_lossy(&repository.stderr).contains("not a git repository") {
            return Ok((
                Changes {
                    repository: false,
                    branch: String::new(),
                    entries: vec![],
                    limited: false,
                },
                false,
            ));
        }
        return Err(io::Error::other(
            String::from_utf8_lossy(&repository.stderr)
                .trim()
                .to_owned(),
        ));
    }
    if text(repository)?.trim() != "true" {
        return Err(io::Error::other("This directory is not a Git working tree"));
    }
    let prefix = text(run(root, &["rev-parse", "--show-prefix"], deadline, 8192)?)?;
    let prefix = prefix.trim_end_matches(['\n', '\r']);
    let stdout = text(run(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--",
            ".",
        ],
        deadline,
        limit,
    )?)?;
    // Such a workspace is one record for the workspace path, which `parse` drops as empty.
    let root_untracked = !prefix.is_empty()
        && stdout
            .split('\0')
            .any(|record| record.strip_prefix("?? ") == Some(prefix));
    let mut entries = parse(&stdout, prefix)?;
    let branch = run(
        root,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        deadline,
        8192,
    )?;
    let branch = if branch.status.success() {
        text(branch)?
    } else {
        text(run(
            root,
            &["rev-parse", "--short", "HEAD"],
            deadline,
            8192,
        )?)?
    };
    let mut collapsed = false;
    if expand_untracked {
        (entries, collapsed) =
            expand_untracked_directories(root, prefix, entries, root_untracked, deadline, limit);
    }
    let limited = collapsed || entries.len() > ENTRY_LIMIT;
    entries.truncate(ENTRY_LIMIT);
    Ok((
        Changes {
            repository: true,
            branch: branch.trim_end_matches(['\n', '\r']).to_owned(),
            entries,
            limited,
        },
        root_untracked,
    ))
}

/// Git reports a new directory as one entry, while editors list each new file in it. List
/// those files too with what is left of the budget. A directory whose files do not fit in the
/// time or output bounds keeps its single entry, as before, and the returned flag is set so
/// the list is not mistaken for a complete one.
fn expand_untracked_directories(
    root: &Path,
    prefix: &str,
    entries: Vec<Change>,
    root_untracked: bool,
    deadline: Instant,
    limit: usize,
) -> (Vec<Change>, bool) {
    let mut directories: HashSet<String> = entries
        .iter()
        .filter(|entry| entry.untracked && entry.directory)
        .map(|entry| entry.path.clone())
        .collect();
    // A workspace that is itself a new directory owns its files under the empty path.
    if root_untracked {
        directories.insert(String::new());
    }
    if directories.is_empty() {
        return (entries, false);
    }
    // One walk of the workspace avoids a command line that grows with the directory count.
    let args = [
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--",
        ".",
    ];
    let Some(files) = run(root, &args, deadline, limit)
        .and_then(text)
        .and_then(|stdout| parse(&stdout, prefix))
        .ok()
    else {
        return (entries, true);
    };
    let mut by_directory: HashMap<String, Vec<Change>> = HashMap::new();
    for file in files.into_iter().filter(|file| file.untracked) {
        // Directories reported in normal mode never nest, so one ancestor owns each file.
        // A nested repository is listed as itself in both modes.
        let owner = std::iter::once(file.path.as_str())
            .chain(file.path.match_indices('/').map(|(at, _)| &file.path[..at]))
            .chain([""])
            .find(|candidate| directories.contains(*candidate))
            .map(str::to_owned);
        if let Some(owner) = owner {
            by_directory.entry(owner).or_default().push(file);
        }
    }
    let mut expanded = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.untracked && entry.directory {
            // The listing is the newer view, so a directory it no longer shows has nothing
            // untracked left: it was removed, emptied or ignored in between.
            expanded.extend(by_directory.remove(&entry.path).unwrap_or_default());
        } else {
            expanded.push(entry);
        }
    }
    expanded.extend(by_directory.remove("").unwrap_or_default());
    (expanded, false)
}

pub fn changes(group: &GroupDoc) -> io::Result<Changes> {
    inspect(
        &workspace::root(group)?,
        Instant::now() + BUDGET,
        STATUS_LIMIT,
        true,
    )
    .map(|(changes, _)| changes)
}
fn open_in_files() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Open this entry in Files to inspect its contents",
    )
}
pub fn diff(group: &GroupDoc, path: &str, side: DiffSide) -> io::Result<Patch> {
    workspace::safe_relative(path)?;
    let root = workspace::root(group)?;
    let deadline = Instant::now() + BUDGET;
    // Only a current status entry can select a diff. No caller-supplied revision or Git option.
    // Untracked files have no diff, so the directories that hold them need not be expanded.
    let (changes, root_untracked) = inspect(&root, deadline, STATUS_LIMIT, false)?;
    // Everything in a workspace that is itself a new directory is untracked.
    if root_untracked {
        return Err(open_in_files());
    }
    let entry = changes
        .entries
        .iter()
        .find(|entry| {
            entry.path == path
                || (entry.untracked
                    && entry.directory
                    && path.starts_with(&entry.path)
                    && path[entry.path.len()..].starts_with('/'))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "This change is no longer listed; refresh Changes",
            )
        })?;
    if entry.untracked || entry.conflicted || entry.directory {
        return Err(open_in_files());
    }
    let code = if side == DiffSide::Staged {
        entry.index
    } else {
        entry.worktree
    };
    if code == ' ' {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "This side has no changes; refresh Changes",
        ));
    }
    // Disabling rename detection prevents a path inside a scoped subdirectory from pulling
    // the contents of a renamed source outside that workspace into the patch.
    let literal = format!(":(literal){path}");
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
        "--submodule=short",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--relative",
    ];
    if side == DiffSide::Staged {
        args.push("--cached");
    }
    args.extend(["--", &literal]);
    let output = run(&root, &args, deadline, PATCH_LIMIT)?;
    if output.stdout_truncated {
        return Ok(Patch {
            patch: String::new(),
            limited: true,
        });
    }
    let patch = text(output)?;
    let limited = patch.lines().take(10_001).count() > 10_000;
    Ok(Patch {
        patch: if limited { String::new() } else { patch },
        limited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_directories_that_do_not_fit_keep_one_entry_and_mark_the_list_limited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let git = Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .output()
            .expect("git");
        assert!(git.status.success());
        let file = format!("fresh/{}.txt", "a".repeat(80));
        std::fs::create_dir_all(root.join("fresh")).expect("fresh");
        std::fs::write(root.join(&file), "a\n").expect("file");
        let listed = |changes: &Changes| -> Vec<(String, bool)> {
            changes
                .entries
                .iter()
                .map(|entry| (entry.path.clone(), entry.directory))
                .collect()
        };
        // Git's single entry for the directory fits in 64 bytes; the listing of its file does not.
        let (changes, _) = inspect(root, Instant::now() + BUDGET, 64, true).expect("inspect");
        assert_eq!(listed(&changes), [("fresh".to_owned(), true)]);
        assert!(changes.limited);
        // No budget left for the listing has the same result.
        let collapsed = || {
            vec![Change {
                path: "fresh".into(),
                index: '?',
                worktree: '?',
                previous_path: None,
                untracked: true,
                conflicted: false,
                directory: true,
            }]
        };
        let (entries, incomplete) = expand_untracked_directories(
            root,
            "",
            collapsed(),
            false,
            Instant::now(),
            STATUS_LIMIT,
        );
        assert!(incomplete);
        assert!(entries.len() == 1 && entries[0].directory && entries[0].path == "fresh");
        let (changes, _) =
            inspect(root, Instant::now() + BUDGET, STATUS_LIMIT, true).expect("inspect");
        assert_eq!(listed(&changes), [(file.clone(), false)]);
        assert!(!changes.limited && changes.entries[0].untracked);
        // A directory removed between the two Git calls is dropped rather than kept stale.
        let mut entries = collapsed();
        entries.insert(
            0,
            Change {
                path: "gone".into(),
                ..collapsed().remove(0)
            },
        );
        let (entries, incomplete) = expand_untracked_directories(
            root,
            "",
            entries,
            false,
            Instant::now() + BUDGET,
            STATUS_LIMIT,
        );
        assert!(!incomplete);
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, [file.as_str()]);
    }

    #[test]
    fn a_new_workspace_directory_that_does_not_fit_marks_its_empty_list_limited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let git = Command::new("git")
            .args(["init", "-q"])
            .current_dir(temp.path())
            .output()
            .expect("git");
        assert!(git.status.success());
        let root = temp.path().join("project");
        let file = format!("{}.txt", "a".repeat(80));
        std::fs::create_dir_all(&root).expect("project");
        std::fs::write(root.join(&file), "a\n").expect("file");
        // Git's record for the workspace fits in 64 bytes; the listing of its file does not.
        let (changes, root_untracked) =
            inspect(&root, Instant::now() + BUDGET, 64, true).expect("inspect");
        assert!(root_untracked && changes.limited && changes.entries.is_empty());
        let (changes, _) =
            inspect(&root, Instant::now() + BUDGET, STATUS_LIMIT, true).expect("inspect");
        let paths: Vec<&str> = changes
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        assert_eq!(paths, [file.as_str()]);
        assert!(!changes.limited && changes.entries[0].untracked);
    }
}

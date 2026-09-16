//! Read-only, bounded Git inspection for one Group workspace.
use crate::{GroupDoc, workspace};
use serde::{Deserialize, Serialize};
use std::{
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
fn inspect(root: &Path, deadline: Instant) -> io::Result<Changes> {
    let repository = run(
        root,
        &["rev-parse", "--is-inside-work-tree"],
        deadline,
        4096,
    )?;
    if !repository.status.success() {
        // A non-repository is a normal Files state; ownership/permission failures are not.
        if String::from_utf8_lossy(&repository.stderr).contains("not a git repository") {
            return Ok(Changes {
                repository: false,
                branch: String::new(),
                entries: vec![],
                limited: false,
            });
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
        STATUS_LIMIT,
    )?)?;
    let mut entries = parse(&stdout, prefix)?;
    let limited = entries.len() > ENTRY_LIMIT;
    entries.truncate(ENTRY_LIMIT);
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
    Ok(Changes {
        repository: true,
        branch: branch.trim_end_matches(['\n', '\r']).to_owned(),
        entries,
        limited,
    })
}

pub fn changes(group: &GroupDoc) -> io::Result<Changes> {
    inspect(&workspace::root(group)?, Instant::now() + BUDGET)
}
pub fn diff(group: &GroupDoc, path: &str, side: DiffSide) -> io::Result<Patch> {
    workspace::safe_relative(path)?;
    let root = workspace::root(group)?;
    let deadline = Instant::now() + BUDGET;
    // Only a current status entry can select a diff. No caller-supplied revision or Git option.
    let changes = inspect(&root, deadline)?;
    let entry = changes
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "This change is no longer listed; refresh Changes",
            )
        })?;
    if entry.untracked || entry.conflicted || entry.directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open this entry in Files to inspect its contents",
        ));
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
